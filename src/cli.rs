use crate::config::Dirs;
use crate::error::{Error, Result};
use std::path::PathBuf;

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
                blurb: "Attach an agent (claude|pi|hermes|codex)",
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

/// The dashed help flags. Bare `help` is handled separately — only in a
/// subcommand position — because it is a legitimate argument value/name
/// elsewhere (e.g. a repo named `help`).
fn is_help_flag(tok: &str) -> bool {
    matches!(tok, "--help" | "-h")
}

fn is_version(tok: &str) -> bool {
    matches!(tok, "--version" | "-V")
}

pub fn render_root_help() -> String {
    let mut out = String::from("wsx — git-worktree workspace manager\n\n");
    out.push_str("USAGE:\n  wsx [COMMAND]            (no command launches the TUI)\n\n");
    out.push_str("COMMANDS:\n");
    let width = GROUPS.iter().map(|g| g.name.len()).max().unwrap_or(0);
    for g in GROUPS {
        out.push_str(&format!(
            "  {:<width$}   {}\n",
            g.name,
            g.blurb,
            width = width
        ));
    }
    out.push_str("\nRun `wsx <command> --help` for command details.\n");
    out
}

pub fn render_group_help(name: &str) -> String {
    let Some(g) = GROUPS.iter().find(|g| g.name == name) else {
        return render_root_help();
    };
    let mut out = format!("wsx {} — {}\n\n", g.name, g.blurb);
    out.push_str(&format!("USAGE:\n  wsx {} <command> [args]\n\n", g.name));
    out.push_str("COMMANDS:\n");
    let width = g.commands.iter().map(|c| c.usage.len()).max().unwrap_or(0);
    for c in g.commands {
        out.push_str(&format!(
            "  {:<width$}   {}\n",
            c.usage,
            c.blurb,
            width = width
        ));
    }
    out
}

type Args = dyn Iterator<Item = String>;

pub fn render_usage_error(group: Option<&str>, msg: &str) -> String {
    let block = match group {
        Some(g) => render_group_help(g),
        None => render_root_help(),
    };
    format!("error: {msg}\n\n{block}")
}

/// Formats a CLI error for stderr. Usage errors render the matching help
/// block; everything else falls back to a one-line message.
pub fn report_cli_error(e: &Error) -> String {
    match e {
        Error::Usage { group, msg } => render_usage_error(*group, msg),
        other => format!("error: {other}\n"),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum HelpTopic {
    Root,
    Group(&'static str),
}

#[derive(Debug)]
pub enum CliAction {
    Tui {
        select: Option<(String, String)>,
    },
    Help(HelpTopic),
    Version,
    RepoAdd {
        path: PathBuf,
        name: String,
        branch_prefix: String,
    },
    RepoList,
    RepoRemove {
        name: String,
    },
    RepoSetPrefix {
        name: String,
        prefix: String,
    },
    RepoSetBaseBranch {
        name: String,
        value: String,
    },
    RepoSetInstructions {
        name: String,
        source: ValueSource,
    },
    RepoSetSetup {
        name: String,
        source: ValueSource,
    },
    RepoSetArchive {
        name: String,
        source: ValueSource,
    },
    RepoEditSetup {
        name: String,
    },
    RepoEditArchive {
        name: String,
    },
    RepoSetPinnedCommands {
        name: String,
        source: ValueSource,
    },
    RepoEditPinnedCommands {
        name: String,
    },
    RepoSetName {
        name: String,
        new_name: String,
    },
    RepoSetRelatedRepos {
        name: String,
        source: ValueSource,
    },
    RepoEditRelatedRepos {
        name: String,
    },
    ConfigGet {
        key: String,
    },
    ConfigSet {
        key: String,
        source: ValueSource,
    },
    ConfigList,
    ConfigEdit {
        key: String,
    },
    RemoteList,
    RemoteRun {
        name: String,
    },
    SharedList {
        json: bool,
    },
    WorkspaceCreate {
        repo: String,
        name: Option<String>,
        yolo: bool,
        shared: bool,
        agent: Option<String>,
        /// Seed the new workspace's primary agent with this prompt, as if
        /// `wsx agent send` had been run against it immediately after.
        prompt: Option<String>,
    },
    WorkspaceList {
        repo: Option<String>,
    },
    WorkspacePath {
        repo: String,
        name: String,
    },
    WorkspaceRename {
        repo: String,
        name: String,
        new_name: String,
    },
    WorkspaceArchive {
        repo: String,
        name: String,
        keep_worktree: bool,
        force_delete_branch: bool,
    },
    WorkspaceShare {
        repo: String,
        name: String,
        shared: bool,
    },
    SetupInstallSkill,
    SetupWaybar,
    WaybarStatus,
    WaybarMenu,
    WaybarJump {
        repo: String,
        slug: String,
    },
    WaybarMenuEntries,
    WaybarRefreshPrs,
    SetupMenubar,
    MenubarPlugin,
    MenubarJump {
        repo: String,
        slug: String,
    },
    MenubarCopyPath {
        repo: String,
        slug: String,
    },
    MenubarRefresh,
    AgentList,
    AgentSend {
        target: String,
        prompt: String,
        /// `<repo>/<slug>` when addressing an agent in ANOTHER workspace;
        /// `None` means the current workspace (the pre-existing behavior).
        workspace: Option<String>,
    },
    AgentAdd {
        kind: String,
    },
    StatusSet {
        state: String,
        message: Option<String>,
    },
    StatusClear,
    StatusFromHook {
        /// The harness whose event payload is on stdin. `None` falls back to
        /// the resolved workspace's agent kind.
        agent: Option<String>,
    },
    StatusFromNotify {
        /// The harness whose `notify` payload is the trailing positional arg.
        /// `None` falls back to the resolved workspace's agent kind.
        agent: Option<String>,
        /// The raw JSON payload Codex passes as the final argv element. If
        /// multiple bare positional args appear, the last one wins; extra args
        /// are tolerated rather than rejected (unlike `from-hook`) because
        /// `notify` must never fail a turn.
        payload: Option<String>,
    },
    RecapSet {
        goal: Option<String>,
        state: Option<String>,
        next: Option<String>,
        goal_short: Option<String>,
        state_short: Option<String>,
        next_short: Option<String>,
    },
    RecapShow,
    RecapClear,
}

#[derive(Debug)]
pub enum ValueSource {
    Literal(String),
    File(PathBuf),
}

impl ValueSource {
    pub fn from_arg(value: String) -> Self {
        if let Some(path) = value.strip_prefix('@') {
            ValueSource::File(PathBuf::from(path))
        } else {
            ValueSource::Literal(value)
        }
    }

    pub fn resolve(self) -> Result<String> {
        match self {
            ValueSource::Literal(s) => Ok(s),
            ValueSource::File(p) => std::fs::read_to_string(&p)
                .map_err(|e| Error::UserInput(format!("read {}: {e}", p.display()))),
        }
    }
}

fn known_setting_key(k: &str) -> bool {
    matches!(
        k,
        "branch_prefix"
            | "custom_instructions"
            | "process_doctrine"
            | "nerd_fonts"
            | "editor_cmd"
            | "terminal_cmd"
            | "diff_cmd"
            | "lazygit_cmd"
            | "chronox_cmd"
            | "notifications"
            | "theme"
            | "mcp_mirror"
            | "remote_control"
            | "remote_control_sandbox"
            | "pinned_commands"
            | "remotes"
            | "shared_hosts"
            | "dashboard_branch_width"
            | "dashboard_pr_width"
            | "dashboard_sort_mode"
            | "dashboard_blocked_pin_max_age_secs"
            | "coding_agent"
            | "detail_bar_config"
            | "usage_graph_window"
    )
}

pub fn parse_args(args: Vec<String>) -> Result<CliAction> {
    let mut rest: Vec<String> = args.into_iter().skip(1).collect();
    let first = if rest.is_empty() {
        None
    } else {
        Some(rest.remove(0))
    };

    match first.as_deref() {
        None => return Ok(CliAction::Tui { select: None }),
        // Match the literal `help` subcommand before the is_help() flag guard,
        // so `wsx help <group>` resolves the group instead of collapsing to Root.
        Some("help") => {
            let topic = match rest.first().and_then(|s| group_name(s)) {
                Some(g) => HelpTopic::Group(g),
                None => HelpTopic::Root,
            };
            return Ok(CliAction::Help(topic));
        }
        Some(t) if is_help_flag(t) => return Ok(CliAction::Help(HelpTopic::Root)),
        Some(t) if is_version(t) => return Ok(CliAction::Version),
        Some("--select") => {
            let Some(target) = rest.first().cloned() else {
                return Err(Error::Usage {
                    group: None,
                    msg: "--select needs <repo>/<slug>".into(),
                });
            };
            let Some((repo, slug)) = target.split_once('/') else {
                return Err(Error::Usage {
                    group: None,
                    msg: "--select target must be <repo>/<slug>".into(),
                });
            };
            return Ok(CliAction::Tui {
                select: Some((repo.to_string(), slug.to_string())),
            });
        }
        _ => {}
    }

    let group = first.as_deref().expect("None handled above");

    // Per-group help. `--help`/`-h` are flag-style and may appear anywhere
    // (`wsx agent send --help`); bare `help` is only a help request in the
    // subcommand slot (`wsx agent help`), since it is a valid value/name
    // elsewhere (e.g. `wsx repo remove help` removes a repo named `help`).
    let wants_group_help =
        rest.iter().any(|a| is_help_flag(a)) || rest.first().map(|a| a.as_str()) == Some("help");
    if wants_group_help {
        if let Some(g) = group_name(group) {
            return Ok(CliAction::Help(HelpTopic::Group(g)));
        }
    }

    let mut it = rest.into_iter();
    match group {
        "repo" => parse_repo(&mut it).map_err(|e| tag_group(e, group)),
        "config" => parse_config(&mut it).map_err(|e| tag_group(e, group)),
        "remote" => parse_remote(&mut it).map_err(|e| tag_group(e, group)),
        "shared" => parse_shared(&mut it).map_err(|e| tag_group(e, group)),
        "workspace" => parse_workspace(&mut it).map_err(|e| tag_group(e, group)),
        "agent" => parse_agent(&mut it).map_err(|e| tag_group(e, group)),
        "setup" => parse_setup(&mut it).map_err(|e| tag_group(e, group)),
        "status" => parse_status(&mut it).map_err(|e| tag_group(e, group)),
        "recap" => parse_recap(&mut it).map_err(|e| tag_group(e, group)),
        "waybar" => parse_waybar(&mut it).map_err(|e| tag_group(e, group)),
        "menubar" => parse_menubar(&mut it).map_err(|e| tag_group(e, group)),
        other => Err(Error::Usage {
            group: None,
            msg: format!("unknown command: {other}"),
        }),
    }
}

fn tag_group(e: Error, group: &str) -> Error {
    match e {
        Error::Usage { group: None, msg } => Error::Usage {
            group: group_name(group),
            msg,
        },
        other => other,
    }
}

fn parse_repo(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("add") => {
            let path = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo add <path>".into(),
            })?;
            let path = PathBuf::from(path);
            let mut name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut branch_prefix = String::new();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--name" => {
                        name = it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--name needs value".into(),
                        })?
                    }
                    "--prefix" => {
                        branch_prefix = it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--prefix needs value".into(),
                        })?
                    }
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
                }
            }
            Ok(CliAction::RepoAdd {
                path,
                name,
                branch_prefix,
            })
        }
        Some("list") => Ok(CliAction::RepoList),
        Some("remove") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo remove <name>".into(),
            })?;
            Ok(CliAction::RepoRemove { name })
        }
        Some("set-prefix") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-prefix <name> <prefix>".into(),
            })?;
            let prefix = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-prefix <name> <prefix>".into(),
            })?;
            Ok(CliAction::RepoSetPrefix { name, prefix })
        }
        Some("set-base-branch") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-base-branch <name> <ref-or-empty>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-base-branch <name> <ref-or-empty>".into(),
            })?;
            Ok(CliAction::RepoSetBaseBranch { name, value })
        }
        Some("set-instructions") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-instructions <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-instructions <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetInstructions {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("set-setup") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-setup <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-setup <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetSetup {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("set-archive") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-archive <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-archive <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetArchive {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("edit-setup") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo edit-setup <name>".into(),
            })?;
            Ok(CliAction::RepoEditSetup { name })
        }
        Some("edit-archive") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo edit-archive <name>".into(),
            })?;
            Ok(CliAction::RepoEditArchive { name })
        }
        Some("set-pinned-commands") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-pinned-commands <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-pinned-commands <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetPinnedCommands {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("edit-pinned-commands") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo edit-pinned-commands <name>".into(),
            })?;
            Ok(CliAction::RepoEditPinnedCommands { name })
        }
        Some("set-name") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-name <name> <new-name>".into(),
            })?;
            let new_name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-name <name> <new-name>".into(),
            })?;
            Ok(CliAction::RepoSetName { name, new_name })
        }
        Some("set-related-repos") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-related-repos <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-related-repos <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetRelatedRepos {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("edit-related-repos") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo edit-related-repos <name>".into(),
            })?;
            Ok(CliAction::RepoEditRelatedRepos { name })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown repo command: {cmd}"),
                None => "missing repo command".into(),
            },
        }),
    }
}

fn parse_config(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("get") => {
            let key = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config get <key>".into(),
            })?;
            if !known_setting_key(&key) {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("unknown setting key: {key}"),
                });
            }
            Ok(CliAction::ConfigGet { key })
        }
        Some("set") => {
            let key = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config set <key> <value-or-@file>".into(),
            })?;
            if !known_setting_key(&key) {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("unknown setting key: {key}"),
                });
            }
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config set <key> <value-or-@file>".into(),
            })?;
            Ok(CliAction::ConfigSet {
                key,
                source: ValueSource::from_arg(value),
            })
        }
        Some("list") => Ok(CliAction::ConfigList),
        Some("edit") => {
            let key = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config edit <key>".into(),
            })?;
            if !known_setting_key(&key) {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("unknown setting key: {key}"),
                });
            }
            Ok(CliAction::ConfigEdit { key })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown config command: {cmd}"),
                None => "missing config command".into(),
            },
        }),
    }
}

fn parse_remote(it: &mut Args) -> Result<CliAction> {
    match it.next() {
        None => Ok(CliAction::RemoteList),
        Some(name) => Ok(CliAction::RemoteRun { name }),
    }
}

fn parse_shared(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("list") => {
            let mut json = false;
            for arg in &mut *it {
                match arg.as_str() {
                    "--json" => json = true,
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
                }
            }
            Ok(CliAction::SharedList { json })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown shared command: {cmd}"),
                None => "missing shared command".into(),
            },
        }),
    }
}

fn parse_workspace(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("create") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg:
                    "workspace create <repo> [--name <slug>] [--yolo] [--shared] [--agent claude|pi|hermes|codex] [--prompt <text>]"
                        .into(),
            })?;
            let mut name: Option<String> = None;
            let mut yolo = false;
            let mut shared = false;
            let mut agent: Option<String> = None;
            let mut prompt: Option<String> = None;
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--name" => {
                        name = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--name needs value".into(),
                        })?);
                    }
                    "--prompt" => {
                        prompt = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--prompt needs value (the text to seed the agent with)".into(),
                        })?);
                    }
                    "--yolo" => yolo = true,
                    "--shared" => shared = true,
                    "--agent" => {
                        agent = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--agent needs value (claude, pi, hermes, or codex)".into(),
                        })?);
                    }
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
                }
            }
            if let Some(ref a) = agent
                && a != "pi"
                && a != "claude"
                && a != "hermes"
                && a != "codex"
            {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("--agent must be 'claude', 'pi', 'hermes', or 'codex', got '{a}'"),
                });
            }
            Ok(CliAction::WorkspaceCreate {
                repo,
                name,
                yolo,
                shared,
                agent,
                prompt,
            })
        }
        Some("list") => {
            let repo = it.next();
            Ok(CliAction::WorkspaceList { repo })
        }
        Some("path") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace path <repo> <name>".into(),
            })?;
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace path <repo> <name>".into(),
            })?;
            Ok(CliAction::WorkspacePath { repo, name })
        }
        Some("rename") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace rename <repo> <name> <new-name>".into(),
            })?;
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace rename <repo> <name> <new-name>".into(),
            })?;
            let new_name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace rename <repo> <name> <new-name>".into(),
            })?;
            Ok(CliAction::WorkspaceRename {
                repo,
                name,
                new_name,
            })
        }
        Some("archive") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace archive <repo> <name> [--keep-worktree] [--force-delete-branch]"
                    .into(),
            })?;
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace archive <repo> <name> [--keep-worktree] [--force-delete-branch]"
                    .into(),
            })?;
            let mut keep_worktree = false;
            let mut force_delete_branch = false;
            for arg in &mut *it {
                match arg.as_str() {
                    "--keep-worktree" => keep_worktree = true,
                    "--force-delete-branch" => force_delete_branch = true,
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
                }
            }
            Ok(CliAction::WorkspaceArchive {
                repo,
                name,
                keep_worktree,
                force_delete_branch,
            })
        }
        Some("share") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace share <repo> <name>".into(),
            })?;
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace share <repo> <name>".into(),
            })?;
            Ok(CliAction::WorkspaceShare {
                repo,
                name,
                shared: true,
            })
        }
        Some("unshare") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace unshare <repo> <name>".into(),
            })?;
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace unshare <repo> <name>".into(),
            })?;
            Ok(CliAction::WorkspaceShare {
                repo,
                name,
                shared: false,
            })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown workspace command: {cmd}"),
                None => "missing workspace command".into(),
            },
        }),
    }
}

const USAGE_AGENT_SEND: &str = "agent send [--workspace <repo>/<slug>] <label> <prompt>";

fn parse_agent(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("list") => Ok(CliAction::AgentList),
        Some("send") => {
            let mut workspace: Option<String> = None;
            // Flags are recognised ONLY before the label. Everything from the
            // label onward is positional, so a message body that itself starts
            // with `--` is preserved verbatim.
            let target = loop {
                let arg = it.next().ok_or_else(|| Error::Usage {
                    group: None,
                    msg: USAGE_AGENT_SEND.into(),
                })?;
                match arg.as_str() {
                    "--workspace" => {
                        workspace = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--workspace needs value (<repo>/<slug>)".into(),
                        })?);
                    }
                    _ => break arg,
                }
            };
            let rest: Vec<String> = it.collect();
            if rest.is_empty() {
                return Err(Error::Usage {
                    group: None,
                    msg: USAGE_AGENT_SEND.into(),
                });
            }
            Ok(CliAction::AgentSend {
                target,
                prompt: rest.join(" "),
                workspace,
            })
        }
        Some("add") => {
            let kind = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "agent add <kind>".into(),
            })?;
            // Validate against the canonical agent set so this can't drift
            // from `AgentKind` as kinds are added/renamed.
            use crate::pty::session::AgentKind;
            if !AgentKind::ALL.iter().any(|k| k.display_name() == kind) {
                let valid = AgentKind::ALL
                    .iter()
                    .map(|k| k.display_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(Error::Usage {
                    group: None,
                    msg: format!("agent add: kind must be one of [{valid}], got '{kind}'"),
                });
            }
            Ok(CliAction::AgentAdd { kind })
        }
        _ => Err(Error::Usage {
            group: None,
            msg: "agent <list|send|add> ...".into(),
        }),
    }
}

fn parse_setup(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("install-skill") => Ok(CliAction::SetupInstallSkill),
        Some("waybar") => Ok(CliAction::SetupWaybar),
        Some("menubar") => Ok(CliAction::SetupMenubar),
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown setup command: {cmd}"),
                None => "missing setup command".into(),
            },
        }),
    }
}

fn parse_waybar(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("status") => Ok(CliAction::WaybarStatus),
        Some("menu") => Ok(CliAction::WaybarMenu),
        Some("jump") => {
            let (Some(repo), Some(slug)) = (it.next(), it.next()) else {
                return Err(Error::Usage {
                    group: None,
                    msg: "jump needs <repo> <slug>".into(),
                });
            };
            Ok(CliAction::WaybarJump { repo, slug })
        }
        Some("menu-entries") => {
            // The installed elephant Lua invokes `menu-entries --json`; make
            // that contract explicit rather than relying on trailing args
            // being silently ignored. Any other trailing arg keeps today's
            // lenient behavior (not rejected).
            let _ = it.next().filter(|a| a == "--json");
            Ok(CliAction::WaybarMenuEntries)
        }
        Some("refresh-prs") => Ok(CliAction::WaybarRefreshPrs),
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown waybar command: {cmd}"),
                None => "missing waybar command".into(),
            },
        }),
    }
}

fn parse_menubar(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("plugin") => Ok(CliAction::MenubarPlugin),
        Some("jump") => {
            let (Some(repo), Some(slug)) = (it.next(), it.next()) else {
                return Err(Error::Usage {
                    group: None,
                    msg: "jump needs <repo> <slug>".into(),
                });
            };
            Ok(CliAction::MenubarJump { repo, slug })
        }
        Some("copy-path") => {
            let (Some(repo), Some(slug)) = (it.next(), it.next()) else {
                return Err(Error::Usage {
                    group: None,
                    msg: "copy-path needs <repo> <slug>".into(),
                });
            };
            Ok(CliAction::MenubarCopyPath { repo, slug })
        }
        Some("refresh") => Ok(CliAction::MenubarRefresh),
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown menubar command: {cmd}"),
                None => "missing menubar command".into(),
            },
        }),
    }
}

fn parse_status(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("set") => {
            let state = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "usage: wsx status set <working|waiting|blocked|done> [--message <text>]"
                    .into(),
            })?;
            let mut message = None;
            while let Some(arg) = it.next() {
                if arg == "--message" || arg == "-m" {
                    message = Some(it.next().ok_or_else(|| Error::Usage {
                        group: None,
                        msg: "--message requires a value".into(),
                    })?);
                } else {
                    return Err(Error::Usage {
                        group: None,
                        msg: format!("unexpected argument: {arg}"),
                    });
                }
            }
            Ok(CliAction::StatusSet { state, message })
        }
        Some("clear") => Ok(CliAction::StatusClear),
        Some("from-hook") => {
            let mut agent = None;
            while let Some(arg) = it.next() {
                if arg == "--agent" {
                    agent = Some(it.next().ok_or_else(|| Error::Usage {
                        group: None,
                        msg: "--agent requires a value".into(),
                    })?);
                } else {
                    return Err(Error::Usage {
                        group: None,
                        msg: format!("unexpected argument: {arg}"),
                    });
                }
            }
            Ok(CliAction::StatusFromHook { agent })
        }
        Some("from-notify") => {
            let mut agent = None;
            let mut payload = None;
            while let Some(arg) = it.next() {
                if arg == "--agent" {
                    agent = Some(it.next().ok_or_else(|| Error::Usage {
                        group: None,
                        msg: "--agent requires a value".into(),
                    })?);
                } else {
                    // Codex appends the JSON payload as the final positional arg.
                    payload = Some(arg);
                }
            }
            Ok(CliAction::StatusFromNotify { agent, payload })
        }
        other => Err(Error::Usage {
            group: None,
            msg: format!("unknown status subcommand: {}", other.unwrap_or("(none)")),
        }),
    }
}

fn parse_recap(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("set") => {
            let mut goal = None;
            let mut state = None;
            let mut next = None;
            let mut goal_short = None;
            let mut state_short = None;
            let mut next_short = None;
            while let Some(arg) = it.next() {
                let slot = match arg.as_str() {
                    "--goal" => &mut goal,
                    "--state" => &mut state,
                    "--next" => &mut next,
                    "--goal-short" => &mut goal_short,
                    "--state-short" => &mut state_short,
                    "--next-short" => &mut next_short,
                    _ => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unexpected argument: {arg}"),
                        });
                    }
                };
                *slot = Some(it.next().ok_or_else(|| Error::Usage {
                    group: None,
                    msg: format!("{arg} requires a value"),
                })?);
            }
            if [&goal, &state, &next, &goal_short, &state_short, &next_short]
                .iter()
                .all(|o| o.is_none())
            {
                return Err(Error::Usage {
                    group: None,
                    msg: "usage: wsx recap set [--goal|--state|--next <text>] \
                          [--goal-short|--state-short|--next-short <text>] (at least one)"
                        .into(),
                });
            }
            Ok(CliAction::RecapSet {
                goal,
                state,
                next,
                goal_short,
                state_short,
                next_short,
            })
        }
        Some("show") => Ok(CliAction::RecapShow),
        Some("clear") => Ok(CliAction::RecapClear),
        other => Err(Error::Usage {
            group: None,
            msg: format!("unknown recap subcommand: {}", other.unwrap_or("(none)")),
        }),
    }
}

pub async fn run_cli(action: CliAction, dirs: &Dirs) -> Result<()> {
    // Actions that don't need the wsx store run before we open it, so a
    // pure `wsx setup install-skill` on a fresh machine doesn't create
    // `~/.local/state/wsx/state.db` as a side effect.
    match &action {
        CliAction::Help(topic) => {
            match topic {
                HelpTopic::Root => print!("{}", render_root_help()),
                HelpTopic::Group(g) => print!("{}", render_group_help(g)),
            }
            return Ok(());
        }
        CliAction::Version => {
            println!("wsx {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }
    if matches!(action, CliAction::SetupInstallSkill) {
        let targets = crate::agent::skill::default_install_targets().ok_or_else(|| {
            Error::UserInput("could not resolve home directory for skill install".into())
        })?;
        for target in targets {
            let outcome = crate::agent::skill::install_to(&target)?;
            let path = target.path.display();
            let skill = target.skill;
            match outcome {
                crate::agent::skill::InstallOutcome::Created => {
                    println!("installed {skill} skill for {} to {path}", target.agent);
                }
                crate::agent::skill::InstallOutcome::Updated => {
                    println!("updated {skill} skill for {} at {path}", target.agent);
                }
                crate::agent::skill::InstallOutcome::Unchanged => {
                    println!(
                        "{skill} skill for {} already up to date at {path}",
                        target.agent
                    );
                }
            }
        }
        return Ok(());
    }
    if matches!(action, CliAction::WaybarStatus) {
        #[cfg(target_os = "linux")]
        {
            crate::waybar::status::print_status(&dirs.db_path());
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        return Err(waybar_linux_only());
    }
    if matches!(action, CliAction::SetupWaybar) {
        #[cfg(target_os = "linux")]
        {
            for line in crate::waybar::install::run()? {
                println!("{line}");
            }
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        return Err(waybar_linux_only());
    }
    if matches!(action, CliAction::MenubarPlugin) {
        #[cfg(target_os = "macos")]
        {
            crate::menubar::plugin::print_plugin(&dirs.db_path());
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        return Err(menubar_macos_only());
    }
    if matches!(action, CliAction::SetupMenubar) {
        #[cfg(target_os = "macos")]
        {
            for line in crate::menubar::install::run()? {
                println!("{line}");
            }
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        return Err(menubar_macos_only());
    }
    let store = crate::data::store::Store::open(&dirs.db_path())?;
    match action {
        CliAction::Tui { .. } => unreachable!("handled in main"),
        CliAction::RepoAdd {
            path,
            name,
            branch_prefix,
        } => {
            crate::data::repo::add(&store, &path, &name, &branch_prefix).await?;
            println!("added repo: {name}");
        }
        CliAction::RepoList => {
            for r in crate::data::repo::list(&store)? {
                println!("{:<20} {}", r.name, r.path.display());
            }
        }
        CliAction::RepoRemove { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            crate::data::repo::remove(&store, r.id)?;
            println!("removed repo: {name}");
        }
        CliAction::RepoSetPrefix { name, prefix } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            store.set_repo_branch_prefix(r.id, &prefix)?;
            if prefix.is_empty() {
                println!("cleared branch prefix for {name} (using global default)");
            } else {
                println!("set branch prefix for {name} to {prefix}");
            }
        }
        CliAction::RepoSetBaseBranch { name, value } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let trimmed = value.trim();
            if trimmed.is_empty() {
                store.set_repo_base_branch(r.id, None)?;
                println!("cleared base branch for {name} (using current HEAD)");
            } else {
                store.set_repo_base_branch(r.id, Some(trimmed))?;
                println!("set base branch for {name} to {trimmed}");
            }
        }
        CliAction::RepoSetInstructions { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_custom_instructions(r.id, None)?;
                println!("cleared custom instructions for {name}");
            } else {
                store.set_repo_custom_instructions(r.id, Some(&value))?;
                println!("set custom instructions for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoSetSetup { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_setup_script(r.id, None)?;
                println!("cleared setup for {name}");
            } else {
                store.set_repo_setup_script(r.id, Some(&value))?;
                println!("set setup for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoSetArchive { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_archive_script(r.id, None)?;
                println!("cleared archive for {name}");
            } else {
                store.set_repo_archive_script(r.id, Some(&value))?;
                println!("set archive for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoEditSetup { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.setup_script.clone().unwrap_or_default();
            let new_value = open_in_editor("setup", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_setup_script(r.id, None)?;
                println!("cleared setup for {name}");
            } else if new_value == current {
                println!("setup unchanged");
            } else {
                store.set_repo_setup_script(r.id, Some(&new_value))?;
                println!("set setup for {name} ({} chars)", new_value.len());
            }
        }
        CliAction::RepoEditArchive { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.archive_script.clone().unwrap_or_default();
            let new_value = open_in_editor("archive", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_archive_script(r.id, None)?;
                println!("cleared archive for {name}");
            } else if new_value == current {
                println!("archive unchanged");
            } else {
                store.set_repo_archive_script(r.id, Some(&new_value))?;
                println!("set archive for {name} ({} chars)", new_value.len());
            }
        }
        CliAction::RepoSetPinnedCommands { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_pinned_commands(r.id, None)?;
                println!("cleared pinned commands for {name}");
            } else {
                store.set_repo_pinned_commands(r.id, Some(&value))?;
                println!("set pinned commands for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoEditPinnedCommands { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.pinned_commands.clone().unwrap_or_default();
            let new_value = open_in_editor("pinned-commands", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_pinned_commands(r.id, None)?;
                println!("cleared pinned commands for {name}");
            } else if new_value == current {
                println!("pinned commands unchanged");
            } else {
                store.set_repo_pinned_commands(r.id, Some(&new_value))?;
                println!("set pinned commands for {name} ({} chars)", new_value.len());
            }
        }
        CliAction::RepoSetName { name, new_name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let trimmed = new_name.trim();
            store.set_repo_name(r.id, trimmed)?;
            println!("renamed repo {name} to {trimmed}");
        }
        CliAction::RepoSetRelatedRepos { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_related_repos(r.id, None)?;
                println!("cleared related repos for {name}");
            } else {
                store.set_repo_related_repos(r.id, Some(&value))?;
                println!("set related repos for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoEditRelatedRepos { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.related_repos.clone().unwrap_or_default();
            let new_value = open_in_editor("related-repos", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_related_repos(r.id, None)?;
                println!("cleared related repos for {name}");
            } else if new_value == current {
                println!("related repos unchanged");
            } else {
                store.set_repo_related_repos(r.id, Some(&new_value))?;
                println!("set related repos for {name} ({} chars)", new_value.len());
            }
        }
        CliAction::ConfigGet { key } => match store.get_setting(&key)? {
            Some(v) => println!("{v}"),
            None => println!("(unset)"),
        },
        CliAction::ConfigSet { key, source } => {
            let value = source.resolve()?;
            if value.is_empty() {
                store.delete_setting(&key)?;
                println!("cleared {key}");
            } else {
                let value = if key == "detail_bar_config" {
                    detail_bar_config_validate_and_normalize(&value)?
                } else if key == "usage_graph_window" {
                    usage_window_validate_and_normalize(&value)?
                } else {
                    value
                };
                store.set_setting(&key, &value)?;
                println!("set {key} ({} chars)", value.len());
            }
        }
        CliAction::ConfigList => {
            let settings = store.list_settings()?;
            if settings.is_empty() {
                println!("(no settings)");
                return Ok(());
            }
            for (k, v) in settings {
                let preview = if v.len() > 60 {
                    format!("{}…", &v[..57])
                } else {
                    v.clone()
                };
                println!("{:<20} {}", k, preview);
            }
        }
        CliAction::ConfigEdit { key } => {
            let current = store.get_setting(&key)?.unwrap_or_default();
            let seed = if key == "detail_bar_config" && current.is_empty() {
                detail_bar_config_seed_for_empty()
            } else {
                current.clone()
            };
            let new_value = open_in_editor(&key, &seed)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.is_empty() {
                store.delete_setting(&key)?;
                println!("cleared {key}");
            } else if new_value == current {
                println!("{key} unchanged");
            } else {
                let normalized = if key == "detail_bar_config" {
                    detail_bar_config_validate_and_normalize(&new_value)?
                } else if key == "usage_graph_window" {
                    usage_window_validate_and_normalize(&new_value)?
                } else {
                    new_value.clone()
                };
                store.set_setting(&key, &normalized)?;
                println!("set {key} ({} chars)", normalized.len());
            }
        }
        CliAction::RemoteList => {
            let remotes = crate::commands::remotes::list(&store)?;
            if remotes.is_empty() {
                println!("no remotes configured. add one with: wsx config edit remotes");
                return Ok(());
            }
            for r in remotes {
                println!("{}", r.name);
            }
        }
        CliAction::RemoteRun { name } => {
            let command = crate::commands::remotes::lookup(&store, &name)?.ok_or_else(|| {
                let available = crate::commands::remotes::list(&store)
                    .ok()
                    .map(|v| v.into_iter().map(|r| r.name).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                if available.is_empty() {
                    Error::UserInput(format!(
                        "no remote named '{name}'. no remotes configured \
                         (add one with: wsx config edit remotes)"
                    ))
                } else {
                    Error::UserInput(format!("no remote named '{name}'. available: {available}"))
                }
            })?;
            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .exec();
            // exec only returns on failure.
            return Err(Error::UserInput(format!("exec sh: {err}")));
        }
        CliAction::SharedList { json } => {
            let mut records = crate::commands::shared::shared_list_records(
                &store,
                crate::pty::tmux::has_session,
            )?;
            // Colorable PR status is only useful to a remote picker consuming
            // `--json`; the human table below doesn't render it, so skip the
            // per-workspace `gh` calls for the plain path.
            if json {
                crate::commands::shared::enrich_with_pr_status(&mut records).await;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&records)?);
            } else if records.is_empty() {
                println!("no shared workspaces");
            } else {
                for rec in &records {
                    if rec.agents.is_empty() {
                        println!("{}\t{}\t(no agents)\t-", rec.repo, rec.workspace);
                        continue;
                    }
                    for agent in &rec.agents {
                        let session = agent.tmux_session.as_deref().unwrap_or("-");
                        let alive = match (agent.alive, &agent.tmux_session) {
                            (true, _) => "alive",
                            (false, Some(_)) => "(dead)",
                            (false, None) => "-",
                        };
                        println!("{}\t{}\t{}\t{}", rec.repo, rec.workspace, session, alive);
                    }
                }
            }
        }
        CliAction::WorkspaceCreate {
            repo,
            name,
            yolo,
            shared,
            agent,
            prompt,
        } => {
            let r = lookup_repo(&store, &repo)?;
            let worktree_base = dirs.app_dir().join("worktrees");
            std::fs::create_dir_all(&worktree_base)?;
            // Inherit yolo + agent kind from the workspace this command runs
            // inside (agent handoffs, or a human in a worktree shell); creates
            // from outside any workspace behave as before.
            let parent = resolve_current_workspace(&store).ok();
            let default_agent = crate::pty::session::AgentKind::from_store(&store);
            let (effective_yolo, agent_kind) =
                effective_create_flags(yolo, agent.as_deref(), parent.as_ref(), default_agent);
            let created = crate::data::workspace::create(
                &store,
                &r,
                name.as_deref(),
                &worktree_base,
                effective_yolo,
                shared,
                agent_kind,
                tokio_util::sync::CancellationToken::new(),
                |_| {},
            )
            .await?;
            println!(
                "created workspace {}/{} at {}",
                r.name,
                created.workspace.name,
                created.workspace.worktree_path.display()
            );
            if let Some(p) = &parent {
                let mut inherited: Vec<String> = Vec::new();
                if effective_yolo && !yolo {
                    inherited.push("yolo".to_string());
                }
                if agent.is_none() && p.agent != default_agent {
                    inherited.push(format!("agent={}", p.agent.display_name()));
                }
                if !inherited.is_empty() {
                    let parent_repo = crate::data::repo::list(&store)?
                        .into_iter()
                        .find(|pr| pr.id == p.repo_id)
                        .map(|pr| pr.name)
                        .unwrap_or_else(|| "(unknown repo)".to_string());
                    println!(
                        "inherited {} from {}/{}",
                        inherited.join(", "),
                        parent_repo,
                        p.name
                    );
                }
            }
            if let crate::data::setup::SetupResult::Failed { exit_code } = created.setup_result {
                println!("warning: setup script exited with code {exit_code}");
            }
            // Seed the agent LAST: `create` above already awaited the setup
            // script, and the dashboard skips workspaces whose setup hasn't
            // finished, so queueing here can't land on a workspace that isn't
            // ready to spawn.
            if let Some(prompt) = prompt.as_deref() {
                let ws_id = created.workspace.id;
                // `create` seeds a primary agent row at birth, so this
                // resolves immediately — but report rather than unwrap, since
                // the workspace itself already exists on disk either way.
                // Every failure from here on must reach the recovery arm
                // below: the worktree already exists, so propagating with `?`
                // would abort with a bare error and no way to resend.
                let seeded = store
                    .primary_instance_id(ws_id)
                    .and_then(|found| {
                        found.ok_or_else(|| {
                            Error::UserInput("new workspace has no primary agent".to_string())
                        })
                    })
                    .and_then(|target| enqueue_for_agent(&store, ws_id, target, prompt));
                match seeded {
                    Ok(()) => println!("queued starter prompt to primary"),
                    // The worktree is live and the prompt is not. Hand back a
                    // command that actually resends THIS prompt, rather than
                    // leaving a workspace that looks created but never wakes.
                    Err(e) => {
                        eprintln!(
                            "warning: workspace created but the starter prompt was not queued: {e}\n\
                             retry with: {}",
                            retry_send_hint(&r.name, &created.workspace.name, prompt)
                        );
                    }
                }
            }
        }
        CliAction::WorkspaceList { repo } => {
            let filtered = match repo {
                Some(name) => vec![lookup_repo(&store, &name)?],
                None => crate::data::repo::list(&store)?,
            };
            for r in filtered {
                for w in store.workspaces(r.id)? {
                    println!(
                        "{}\t{}\t{}\t{}",
                        r.name,
                        w.name,
                        w.branch,
                        w.worktree_path.display()
                    );
                }
            }
        }
        CliAction::WorkspacePath { repo, name } => {
            let r = lookup_repo(&store, &repo)?;
            let w = lookup_workspace(&store, &r, &name)?;
            println!("{}", w.worktree_path.display());
        }
        CliAction::WorkspaceRename {
            repo,
            name,
            new_name,
        } => {
            let r = lookup_repo(&store, &repo)?;
            let w = lookup_workspace(&store, &r, &name)?;
            if new_name == name {
                println!("workspace {}/{} unchanged", r.name, name);
            } else {
                crate::data::workspace::rename(&store, &r, &w, &new_name).await?;
                println!(
                    "renamed workspace {}/{} to {}/{}",
                    r.name, name, r.name, new_name
                );
            }
        }
        CliAction::WorkspaceArchive {
            repo,
            name,
            keep_worktree,
            force_delete_branch,
        } => {
            let r = lookup_repo(&store, &repo)?;
            let w = lookup_workspace(&store, &r, &name)?;
            let opts = crate::data::workspace::ArchiveOpts {
                keep_worktree,
                force_branch_delete: force_delete_branch,
            };
            crate::data::workspace::archive(&store, &r, &w, opts, |_| {}).await?;
            println!("archived workspace {}/{}", r.name, name);
        }
        CliAction::WorkspaceShare { repo, name, shared } => {
            let r = lookup_repo(&store, &repo)?;
            let w = lookup_workspace(&store, &r, &name)?;
            if w.shared == shared {
                println!(
                    "workspace {}/{} already {}",
                    r.name,
                    name,
                    if shared { "shared" } else { "unshared" }
                );
            } else {
                store.set_workspace_shared(w.id, shared)?;
                println!(
                    "workspace {}/{} is now {}",
                    r.name,
                    name,
                    if shared { "shared" } else { "unshared" }
                );
                println!("note: running sessions keep their current backend until restarted");
            }
        }
        CliAction::AgentList => {
            let ws = resolve_current_workspace(&store)?;
            for inst in store.workspace_agents(ws.id)? {
                let tag = if inst.is_primary { "  (primary)" } else { "" };
                println!("{}  {}{}", inst.id.0, inst.label(), tag);
            }
        }
        CliAction::AgentSend {
            target,
            prompt,
            workspace,
        } => {
            let target_ws = match workspace.as_deref() {
                Some(spec) => resolve_workspace_spec(&store, spec)?,
                None => resolve_current_workspace(&store)?,
            };
            let target_id = store
                .resolve_instance_label(target_ws.id, &target)?
                .ok_or_else(|| {
                    // `wsx agent list` only reports the CURRENT workspace, so
                    // list the target's labels inline instead of pointing at it.
                    let labels = store
                        .workspace_agents(target_ws.id)
                        .map(|v| {
                            let names: Vec<String> = v.iter().map(|i| i.label()).collect();
                            join_or_none(names.iter().map(|s| s.as_str()))
                        })
                        .unwrap_or_else(|_| "(unknown)".to_string());
                    Error::UserInput(format!(
                        "no agent '{target}' in workspace {}; agents there: {labels} \
                         (or `primary` for whichever is that workspace's primary agent)",
                        target_ws.name
                    ))
                })?;
            enqueue_for_agent(&store, target_ws.id, target_id, &prompt)?;
            match workspace.as_deref() {
                Some(_) => println!("queued message to {target} in {}", target_ws.name),
                None => println!("queued message to {target}"),
            }
        }
        CliAction::AgentAdd { kind } => {
            let ws = resolve_current_workspace(&store)?;
            let agent = crate::pty::session::AgentKind::from_str_or_default(Some(&kind));
            let inst = store.add_workspace_agent(ws.id, agent)?;
            println!("added {}", inst.label());
        }
        CliAction::StatusSet { state, message } => {
            let parsed = crate::data::store::ReportedState::parse(&state).ok_or_else(|| {
                Error::UserInput(format!(
                    "invalid status '{state}'; expected working|waiting|blocked|done"
                ))
            })?;
            let ws = resolve_current_workspace(&store)?;
            store.set_workspace_status(ws.id, parsed, message.as_deref(), "model")?;
            println!("status: {}", parsed.as_str());
        }
        CliAction::StatusClear => {
            let ws = resolve_current_workspace(&store)?;
            store.clear_workspace_status(ws.id)?;
            println!("status cleared");
        }
        CliAction::StatusFromHook { agent } => {
            use std::io::Read;
            let mut buf = String::new();
            // Hooks pipe JSON on stdin; tolerate empty/garbage by no-op exit 0
            // so a hook never fails the agent's turn.
            let _ = std::io::stdin().read_to_string(&mut buf);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&buf) {
                if let Ok(ws) = resolve_current_workspace(&store) {
                    let kind = match &agent {
                        Some(a) => crate::pty::session::AgentKind::from_str_or_default(Some(a)),
                        None => ws.agent,
                    };
                    if let Some(state) = crate::agent::status::for_agent(kind).parse_event(&json) {
                        let _ = store.apply_hook_status(ws.id, state, "hook");
                    }
                }
            }
            // Always succeed: a status hook must never block or fail the turn.
        }
        CliAction::StatusFromNotify { agent, payload } => {
            // Codex `notify` passes JSON as the final argv (not stdin). Tolerate
            // missing/garbage payloads by no-op exit 0 — notify must never fail
            // a turn.
            if let Some(payload) = payload {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&payload) {
                    if let Ok(ws) = resolve_current_workspace(&store) {
                        let kind = match &agent {
                            Some(a) => crate::pty::session::AgentKind::from_str_or_default(Some(a)),
                            None => ws.agent,
                        };
                        if let Some(state) =
                            crate::agent::status::for_agent(kind).parse_event(&json)
                        {
                            let _ = store.apply_hook_status(ws.id, state, "notify");
                        }
                    }
                }
            }
            // Always succeed.
        }
        CliAction::RecapSet {
            goal,
            state,
            next,
            goal_short,
            state_short,
            next_short,
        } => {
            let ws = resolve_current_workspace(&store)?;
            store.set_workspace_recap(
                ws.id,
                goal.as_deref(),
                state.as_deref(),
                next.as_deref(),
                goal_short.as_deref(),
                state_short.as_deref(),
                next_short.as_deref(),
            )?;
            println!("recap updated");
        }
        CliAction::RecapShow => {
            let ws = resolve_current_workspace(&store)?;
            match store.workspace_recap(ws.id)? {
                Some(r) => {
                    println!("goal:        {}", r.goal.as_deref().unwrap_or("-"));
                    println!("state:       {}", r.state.as_deref().unwrap_or("-"));
                    println!("next:        {}", r.next.as_deref().unwrap_or("-"));
                    println!("goal-short:  {}", r.goal_short.as_deref().unwrap_or("-"));
                    println!("state-short: {}", r.state_short.as_deref().unwrap_or("-"));
                    println!("next-short:  {}", r.next_short.as_deref().unwrap_or("-"));
                }
                None => println!("no recap set"),
            }
        }
        CliAction::RecapClear => {
            let ws = resolve_current_workspace(&store)?;
            store.clear_workspace_recap(ws.id)?;
            println!("recap cleared");
        }
        #[cfg(target_os = "linux")]
        CliAction::WaybarMenu => crate::waybar::menu::run_menu(&store)?,
        #[cfg(target_os = "linux")]
        CliAction::WaybarJump { repo, slug } => crate::waybar::jump::jump(&repo, &slug)?,
        #[cfg(target_os = "linux")]
        CliAction::WaybarMenuEntries => crate::waybar::entries::run_menu_entries(&store).await?,
        #[cfg(target_os = "linux")]
        CliAction::WaybarRefreshPrs => crate::waybar::entries::run_refresh_prs(&store).await?,
        #[cfg(not(target_os = "linux"))]
        CliAction::WaybarMenu
        | CliAction::WaybarJump { .. }
        | CliAction::WaybarMenuEntries
        | CliAction::WaybarRefreshPrs => return Err(waybar_linux_only()),
        #[cfg(target_os = "macos")]
        CliAction::MenubarJump { repo, slug } => {
            let terminal_cmd = store.get_setting("terminal_cmd")?;
            crate::menubar::jump::jump(&repo, &slug, terminal_cmd.as_deref())?
        }
        #[cfg(target_os = "macos")]
        CliAction::MenubarCopyPath { repo, slug } => {
            crate::menubar::jump::copy_path(&store, &repo, &slug)?
        }
        #[cfg(target_os = "macos")]
        CliAction::MenubarRefresh => crate::menubar::refresh::run_refresh(&store).await?,
        #[cfg(not(target_os = "macos"))]
        CliAction::MenubarJump { .. }
        | CliAction::MenubarCopyPath { .. }
        | CliAction::MenubarRefresh => {
            return Err(menubar_macos_only());
        }
        CliAction::SetupInstallSkill
        | CliAction::WaybarStatus
        | CliAction::SetupWaybar
        | CliAction::MenubarPlugin
        | CliAction::SetupMenubar => {
            unreachable!("handled before store open")
        }
        CliAction::Help(_) | CliAction::Version => {
            unreachable!("handled before store open")
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn waybar_linux_only() -> Error {
    Error::UserInput("wsx waybar is only available on Linux (waybar integration)".into())
}

#[cfg(not(target_os = "macos"))]
fn menubar_macos_only() -> Error {
    Error::UserInput("wsx menubar is only available on macOS (SwiftBar integration)".into())
}

/// Resolve the workspace the current `wsx` invocation is acting within:
/// prefer the `WSX_WORKSPACE_ID` env var (set when wsx spawns an agent), else
/// fall back to matching the current directory against known worktree paths.
fn resolve_current_workspace(
    store: &crate::data::store::Store,
) -> Result<crate::data::store::Workspace> {
    use crate::data::store::WorkspaceId;
    // 1. WSX_WORKSPACE_ID (reliable for agent-initiated calls)
    if let Ok(s) = std::env::var("WSX_WORKSPACE_ID") {
        if let Ok(id) = s.parse::<i64>() {
            if let Some(ws) = store.workspace_by_id(WorkspaceId(id))? {
                return Ok(ws);
            }
        }
    }
    // 2. cwd: find the workspace whose worktree_path is an ancestor-or-equal of cwd
    // Note: this is a raw path-prefix match. If the user `cd`'d into the
    // worktree through a symlink (e.g. macOS /var -> /private/var), cwd may not
    // prefix the stored worktree_path and the match will miss. Setting
    // WSX_WORKSPACE_ID (the agent-spawn path) avoids this entirely.
    let cwd = std::env::current_dir()
        .map_err(|e| Error::UserInput(format!("cannot determine current directory: {e}")))?;
    let ws = store
        .all_workspaces()?
        .into_iter()
        .filter(|w| cwd.starts_with(&w.worktree_path))
        .max_by_key(|w| w.worktree_path.as_os_str().len())
        .ok_or_else(|| {
            Error::UserInput(
                "not inside a wsx workspace (set WSX_WORKSPACE_ID or run from a worktree)".into(),
            )
        })?;
    Ok(ws)
}

/// Effective yolo + agent for a new workspace: explicit flags win, then the
/// parent workspace (the one this `wsx` invocation runs inside, if any), then
/// `default_agent` (the `coding_agent` setting — the same default the TUI's
/// create modal uses). Inheritance means an agent handing work to a sibling
/// workspace doesn't need to know — and can't reliably know — its own
/// workspace's yolo state or agent kind. Pure so it can be unit-tested
/// without the process-global env/cwd that `resolve_current_workspace` reads.
fn effective_create_flags(
    explicit_yolo: bool,
    explicit_agent: Option<&str>,
    parent: Option<&crate::data::store::Workspace>,
    default_agent: crate::pty::session::AgentKind,
) -> (bool, crate::pty::session::AgentKind) {
    let yolo = explicit_yolo || parent.is_some_and(|p| p.yolo);
    let agent = match explicit_agent {
        Some(_) => crate::pty::session::AgentKind::from_str_or_default(explicit_agent),
        None => match parent {
            Some(p) => p.agent,
            None => default_agent,
        },
    };
    (yolo, agent)
}

fn lookup_repo(store: &crate::data::store::Store, name: &str) -> Result<crate::data::store::Repo> {
    crate::data::repo::list(store)?
        .into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))
}

fn lookup_workspace(
    store: &crate::data::store::Store,
    repo: &crate::data::store::Repo,
    name: &str,
) -> Result<crate::data::store::Workspace> {
    store
        .workspaces(repo.id)?
        .into_iter()
        .find(|w| w.name == name)
        .ok_or_else(|| Error::UserInput(format!("no workspace named {name} in repo {}", repo.name)))
}

/// Resolve a `--workspace <repo>/<slug>` spec to a workspace.
///
/// Splits on the LAST `/`: repo names may contain spaces and other
/// characters, but a workspace slug never contains `/` (the same assumption
/// `tui_ipc::parse_line` makes). Errors list the valid alternatives, because
/// the caller is usually an agent that cannot enumerate them itself.
fn resolve_workspace_spec(
    store: &crate::data::store::Store,
    spec: &str,
) -> Result<crate::data::store::Workspace> {
    let malformed = || Error::UserInput(format!("--workspace expects <repo>/<slug>, got '{spec}'"));
    let (repo_name, slug) = spec.rsplit_once('/').ok_or_else(malformed)?;
    if repo_name.is_empty() || slug.is_empty() {
        return Err(malformed());
    }
    let repos = crate::data::repo::list(store)?;
    let repo = repos.iter().find(|r| r.name == repo_name).ok_or_else(|| {
        Error::UserInput(format!(
            "--workspace: no repo named '{repo_name}'; known repos: {}",
            join_or_none(repos.iter().map(|r| r.name.as_str()))
        ))
    })?;
    let workspaces = store.workspaces(repo.id)?;
    workspaces
        .iter()
        .find(|w| w.name == slug)
        .cloned()
        .ok_or_else(|| {
            Error::UserInput(format!(
                "--workspace: no workspace '{slug}' in repo '{repo_name}'; known: {}",
                join_or_none(workspaces.iter().map(|w| w.name.as_str()))
            ))
        })
}

/// Comma-join names for an error hint, or `(none)` when the list is empty.
fn join_or_none<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let v: Vec<&str> = names.collect();
    if v.is_empty() {
        "(none)".to_string()
    } else {
        v.join(", ")
    }
}

/// The `wsx agent send` invocation that resends a starter prompt whose
/// enqueue failed after the workspace was already created.
///
/// Every dynamic part is shell-quoted: repo names may contain spaces (the
/// `<repo>/<slug>` spec splits on the LAST slash precisely because of that)
/// and a prompt is arbitrary text. An unquoted hint would be a command the
/// user cannot actually paste.
fn retry_send_hint(repo: &str, slug: &str, prompt: &str) -> String {
    fn shquote(s: &str) -> String {
        shlex::try_quote(s)
            .map(|c| c.into_owned())
            // Only fails on interior NUL, which cannot reach here through
            // sqlite TEXT or a CLI arg; drop the byte rather than emit an
            // unquoted arg.
            .unwrap_or_else(|_| format!("'{}'", s.replace(['\'', '\0'], "")))
    }
    format!(
        "wsx agent send --workspace {} primary {}",
        shquote(&format!("{repo}/{slug}")),
        shquote(prompt)
    )
}

/// Queue `body` for `target` and warn when nothing will deliver it.
///
/// The CLI only ever writes to the store; the dashboard is the sole thing
/// that injects queued messages into an agent PTY (`App::drain_agent_messages`
/// spawns the target on demand). So without a live TUI the enqueue is a no-op
/// the sender would never notice — not an error, since the row is queued
/// rather than lost, but worth saying out loud.
///
/// Shared by `agent send` and `workspace create --prompt` so the two can't
/// drift apart on sender attribution or on that warning.
fn enqueue_for_agent(
    store: &crate::data::store::Store,
    workspace: crate::data::store::WorkspaceId,
    target: crate::data::store::AgentInstanceId,
    body: &str,
) -> Result<()> {
    let from = std::env::var("WSX_AGENT_INSTANCE_ID")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .map(crate::data::store::AgentInstanceId);
    store.enqueue_message(workspace, target, from, body)?;
    if !crate::tui_ipc::any_live_tui() {
        eprintln!(
            "warning: no wsx dashboard is running — this message is queued and \
             will not be delivered until one starts. Tell the user to open `wsx`."
        );
    }
    Ok(())
}

fn open_in_editor(key: &str, initial: &str) -> Result<String> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let dir = std::env::temp_dir();
    let path = dir.join(format!("wsx-{key}-{}.txt", std::process::id()));
    std::fs::write(&path, initial)?;
    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| Error::UserInput(format!("spawn editor {editor}: {e}")))?;
    if !status.success() {
        let _ = std::fs::remove_file(&path);
        return Err(Error::UserInput(format!(
            "editor {editor} exited with {status}"
        )));
    }
    let value = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(value)
}

/// Seed text for the editor when the global `detail_bar_config`
/// setting is empty — the pretty-printed default config.
fn detail_bar_config_seed_for_empty() -> String {
    serde_json::to_string_pretty(&crate::config::detail_bar_config::DetailBarConfig::default())
        .unwrap_or_else(|_| "{}".to_string())
}

/// Parse, sanitize, and re-serialize a global `detail_bar_config`
/// blob. Returns the pretty-printed normalized JSON.
fn detail_bar_config_validate_and_normalize(raw: &str) -> Result<String> {
    let mut cfg: crate::config::detail_bar_config::DetailBarConfig = serde_json::from_str(raw)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: invalid JSON: {e}")))?;
    cfg.sanitize();
    serde_json::to_string_pretty(&cfg)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: serialize failed: {e}")))
}

/// Validate a `usage_graph_window` value: accept only the canonical tokens
/// (`24h`/`1w`/`1mo`), ignoring surrounding whitespace, and store the trimmed
/// canonical form. Rejects anything else so a CLI typo fails loudly instead of
/// silently falling back to `24h` at render time.
fn usage_window_validate_and_normalize(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if crate::config::usage_window::UsageWindow::ALL
        .iter()
        .any(|w| w.as_setting() == trimmed)
    {
        Ok(trimmed.to_string())
    } else {
        Err(Error::UserInput(format!(
            "usage_graph_window: expected one of 24h, 1w, 1mo (got {trimmed:?})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<CliAction> {
        let mut v = vec!["wsx".to_string()];
        v.extend(args.iter().map(|s| s.to_string()));
        parse_args(v)
    }

    #[test]
    fn parses_agent_send_with_workspace_flag() {
        match parse(&[
            "agent",
            "send",
            "--workspace",
            "backend/add-widgets",
            "primary",
            "do",
            "the",
            "thing",
        ])
        .unwrap()
        {
            CliAction::AgentSend {
                target,
                prompt,
                workspace,
            } => {
                assert_eq!(target, "primary");
                assert_eq!(prompt, "do the thing");
                assert_eq!(workspace.as_deref(), Some("backend/add-widgets"));
            }
            other => panic!("expected AgentSend, got {other:?}"),
        }
    }

    #[test]
    fn agent_send_flags_are_only_recognised_before_the_label() {
        // Everything from the label onward is body, so a message that itself
        // starts with `--` is preserved verbatim rather than parsed as a flag.
        match parse(&["agent", "send", "claude", "--workspace", "is", "a", "flag"]).unwrap() {
            CliAction::AgentSend {
                target,
                prompt,
                workspace,
            } => {
                assert_eq!(target, "claude");
                assert_eq!(prompt, "--workspace is a flag");
                assert_eq!(workspace, None);
            }
            other => panic!("expected AgentSend, got {other:?}"),
        }
    }

    #[test]
    fn agent_send_rejects_incomplete_invocations() {
        assert!(parse(&["agent", "send", "--workspace"]).is_err()); // flag needs a value
        assert!(parse(&["agent", "send", "--workspace", "backend/x"]).is_err()); // no label
        assert!(parse(&["agent", "send", "--workspace", "backend/x", "primary"]).is_err()); // no body
    }

    fn seed_spec_store() -> crate::data::store::Store {
        use crate::data::store::{NewWorkspace, Store};
        let store = Store::open_in_memory().unwrap();
        // A repo name containing a space exercises the split-on-LAST-slash rule.
        let repo = store
            .add_repo(std::path::Path::new("/tmp/mb"), "meals backend", "wsx")
            .unwrap();
        store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "api-fix",
                branch: "wsx/api-fix",
                worktree_path: std::path::Path::new("/tmp/mb/api-fix"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
    }

    #[test]
    fn workspace_spec_splits_on_the_last_slash() {
        let store = seed_spec_store();
        let ws = resolve_workspace_spec(&store, "meals backend/api-fix").unwrap();
        assert_eq!(ws.name, "api-fix");
    }

    #[test]
    fn workspace_spec_errors_name_the_valid_alternatives() {
        let store = seed_spec_store();

        let e = resolve_workspace_spec(&store, "noslug")
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("<repo>/<slug>"),
            "must show the expected form: {e}"
        );

        let e = resolve_workspace_spec(&store, "/api-fix")
            .unwrap_err()
            .to_string();
        assert!(e.contains("<repo>/<slug>"), "empty repo is malformed: {e}");

        let e = resolve_workspace_spec(&store, "meals backend/")
            .unwrap_err()
            .to_string();
        assert!(e.contains("<repo>/<slug>"), "empty slug is malformed: {e}");

        let e = resolve_workspace_spec(&store, "nope/api-fix")
            .unwrap_err()
            .to_string();
        assert!(e.contains("meals backend"), "must list known repos: {e}");

        let e = resolve_workspace_spec(&store, "meals backend/nope")
            .unwrap_err()
            .to_string();
        assert!(e.contains("api-fix"), "must list known slugs: {e}");
    }

    /// Dispatch-arm coverage for `wsx agent send --workspace`: the target
    /// workspace, its label resolution, and the `enqueue_message` argument
    /// order are the one seam nothing else in the branch exercises.
    #[tokio::test]
    async fn agent_send_dispatch_targets_the_other_workspaces_primary() {
        use crate::config::Dirs;
        use crate::data::store::{NewWorkspace, Store};
        use crate::pty::session::AgentKind;
        use crate::test_support::EnvGuard;

        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::for_test(tmp.path());

        // Seed two workspaces directly against the DB file `run_cli` will
        // open, so we can assert the queued row lands against the TARGET,
        // not the sender's own (origin) workspace.
        let (origin_primary, target_ws, target_primary) = {
            let store = Store::open(&dirs.db_path()).unwrap();
            let repo = store
                .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
                .unwrap();
            let origin = store
                .insert_workspace(&NewWorkspace {
                    repo_id: repo,
                    name: "origin",
                    branch: "wsx/origin",
                    worktree_path: std::path::Path::new("/tmp/r/origin"),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            let origin_primary = store
                .add_primary_agent(origin, AgentKind::Claude, 1)
                .unwrap();

            let target = store
                .insert_workspace(&NewWorkspace {
                    repo_id: repo,
                    name: "target",
                    branch: "wsx/target",
                    worktree_path: std::path::Path::new("/tmp/r/target"),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            let target_primary = store
                .add_primary_agent(target, AgentKind::Claude, 1)
                .unwrap();
            (origin_primary.id, target, target_primary.id)
        };

        let mut env = EnvGuard::new();
        // Point the "is a TUI running" check at an empty scratch dir so the
        // dispatch's stderr warning path is deterministic regardless of the
        // ambient environment (this process may itself be running under a
        // live wsx dashboard).
        env.set("XDG_RUNTIME_DIR", tmp.path());
        // Target resolution must come entirely from `workspace`, not from
        // the sender's own identity, so leave the sender unset.
        env.remove("WSX_AGENT_INSTANCE_ID");

        let action = CliAction::AgentSend {
            target: "primary".to_string(),
            prompt: "do the thing".to_string(),
            workspace: Some("r/target".to_string()),
        };
        run_cli(action, &dirs).await.unwrap();

        let store = Store::open(&dirs.db_path()).unwrap();
        let queued = store.undelivered_messages().unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(
            queued[0].workspace_id, target_ws,
            "must queue against the TARGET workspace"
        );
        assert_eq!(
            queued[0].target_agent_id, target_primary,
            "must resolve `primary` against the TARGET workspace, not the origin"
        );
        assert_ne!(
            queued[0].target_agent_id, origin_primary,
            "must not resolve `primary` against the origin workspace"
        );
        assert_eq!(queued[0].body, "do the thing");
    }

    /// The unknown-label error must offer `primary` alongside the concrete
    /// labels. A cross-workspace sender cannot run `wsx agent list` against
    /// the target, so this error is the one place it learns how to recover —
    /// and `primary` is the label that works whatever kind the target runs.
    #[tokio::test]
    async fn agent_send_unknown_label_error_offers_the_primary_alias() {
        use crate::config::Dirs;
        use crate::data::store::{NewWorkspace, Store};
        use crate::pty::session::AgentKind;
        use crate::test_support::EnvGuard;

        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        {
            let store = Store::open(&dirs.db_path()).unwrap();
            let repo = store
                .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
                .unwrap();
            let ws = store
                .insert_workspace(&NewWorkspace {
                    repo_id: repo,
                    name: "target",
                    branch: "wsx/target",
                    worktree_path: std::path::Path::new("/tmp/r/target"),
                    yolo: false,
                    agent: AgentKind::Hermes,
                    shared: false,
                })
                .unwrap();
            store.add_primary_agent(ws, AgentKind::Hermes, 1).unwrap();
        }

        let mut env = EnvGuard::new();
        env.set("XDG_RUNTIME_DIR", tmp.path());
        env.remove("WSX_AGENT_INSTANCE_ID");

        // Guess the wrong kind label against a hermes-primary workspace —
        // the exact case a sender hits when it cannot enumerate the target.
        let action = CliAction::AgentSend {
            target: "claude".to_string(),
            prompt: "do the thing".to_string(),
            workspace: Some("r/target".to_string()),
        };
        let err = run_cli(action, &dirs).await.unwrap_err().to_string();
        assert!(
            err.contains("hermes"),
            "must list the concrete labels that exist: {err}"
        );
        assert!(
            err.contains("primary"),
            "must offer the primary alias as a recovery path: {err}"
        );
        // Nothing is queued when resolution fails.
        let store = Store::open(&dirs.db_path()).unwrap();
        assert!(store.undelivered_messages().unwrap().is_empty());
    }

    #[test]
    fn misuse_is_tagged_with_group() {
        match parse(&["agent", "send"]) {
            Err(Error::Usage {
                group: Some("agent"),
                ..
            }) => {}
            other => panic!("expected agent-tagged Usage, got {other:?}"),
        }
    }

    #[test]
    fn unknown_command_is_untagged_usage() {
        match parse(&["bogus"]) {
            Err(Error::Usage { group: None, .. }) => {}
            other => panic!("expected untagged Usage, got {other:?}"),
        }
    }

    #[test]
    fn parses_top_level_help_forms() {
        for f in ["--help", "-h", "help"] {
            assert!(matches!(
                parse(&[f]).unwrap(),
                CliAction::Help(HelpTopic::Root)
            ));
        }
    }

    #[test]
    fn parses_version_forms() {
        for f in ["--version", "-V"] {
            assert!(matches!(parse(&[f]).unwrap(), CliAction::Version));
        }
    }

    #[test]
    fn bare_wsx_is_tui() {
        assert!(matches!(parse(&[]).unwrap(), CliAction::Tui { .. }));
    }

    #[test]
    fn parses_select_launch_flag() {
        match parse(&["--select", "meals backend/api-fix"]) {
            Ok(CliAction::Tui {
                select: Some((repo, slug)),
            }) => {
                assert_eq!(repo, "meals backend");
                assert_eq!(slug, "api-fix");
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(matches!(parse(&[]), Ok(CliAction::Tui { select: None })));
        assert!(parse(&["--select"]).is_err());
        assert!(parse(&["--select", "no-slash"]).is_err());
    }

    #[test]
    fn parses_group_help_forms() {
        let want = |a: CliAction| matches!(a, CliAction::Help(HelpTopic::Group("agent")));
        assert!(want(parse(&["agent", "--help"]).unwrap()));
        assert!(want(parse(&["agent", "-h"]).unwrap()));
        assert!(want(parse(&["help", "agent"]).unwrap()));
    }

    #[test]
    fn dashed_help_flag_triggers_group_help_anywhere() {
        let want = |a: CliAction| matches!(a, CliAction::Help(HelpTopic::Group("agent")));
        // After a valid subcommand, a dashed flag still surfaces group help.
        assert!(want(parse(&["agent", "send", "--help"]).unwrap()));
        assert!(want(parse(&["agent", "send", "-h"]).unwrap()));
    }

    /// The ordering settings the dashboard reads must be reachable from
    /// `wsx config set`, or "configurable" is only true for hand-edited SQL.
    #[test]
    fn dashboard_ordering_settings_are_settable_from_the_cli() {
        for key in ["dashboard_sort_mode", "dashboard_blocked_pin_max_age_secs"] {
            match parse(&["config", "set", key, "x"]).unwrap() {
                CliAction::ConfigSet { key: k, .. } => assert_eq!(k, key),
                other => panic!("expected ConfigSet for {key}, got {other:?}"),
            }
        }
    }

    #[test]
    fn bare_help_is_a_subcommand_not_a_value() {
        // `help` in the subcommand slot → group help.
        assert!(matches!(
            parse(&["repo", "help"]).unwrap(),
            CliAction::Help(HelpTopic::Group("repo"))
        ));
        // `help` as an argument VALUE must NOT trigger help.
        match parse(&["repo", "remove", "help"]).unwrap() {
            CliAction::RepoRemove { name } => assert_eq!(name, "help"),
            other => panic!("expected RepoRemove {{ name: \"help\" }}, got {other:?}"),
        }
        match parse(&["config", "set", "editor_cmd", "help"]).unwrap() {
            CliAction::ConfigSet {
                key,
                source: ValueSource::Literal(v),
            } => {
                assert_eq!(key, "editor_cmd");
                assert_eq!(v, "help");
            }
            other => panic!("expected ConfigSet value \"help\", got {other:?}"),
        }
        match parse(&["agent", "send", "claude", "help"]).unwrap() {
            CliAction::AgentSend {
                target,
                prompt,
                workspace,
            } => {
                assert_eq!(target, "claude");
                assert_eq!(prompt, "help");
                assert_eq!(workspace, None);
            }
            other => panic!("expected AgentSend prompt \"help\", got {other:?}"),
        }
    }

    #[test]
    fn help_for_unknown_group_falls_back_to_root() {
        assert!(matches!(
            parse(&["help", "bogus"]).unwrap(),
            CliAction::Help(HelpTopic::Root)
        ));
    }

    #[test]
    fn group_name_resolves_known_and_unknown() {
        assert_eq!(group_name("agent"), Some("agent"));
        assert_eq!(group_name("workspace"), Some("workspace"));
        assert_eq!(group_name("bogus"), None);
    }

    #[test]
    fn root_help_lists_every_group() {
        let h = render_root_help();
        for g in GROUPS {
            assert!(h.contains(g.name), "root help missing group {}", g.name);
        }
        assert!(h.contains("launches the TUI"));
    }

    #[test]
    fn agent_group_help_lists_its_commands() {
        let h = render_group_help("agent");
        assert!(h.contains("list"));
        assert!(h.contains("add <kind>"));
        assert!(h.contains("send [--workspace <repo>/<slug>] <label> <message...>"));
    }

    #[test]
    fn usage_error_has_message_then_group_block() {
        let s = render_usage_error(Some("agent"), "missing arguments");
        assert!(s.starts_with("error: missing arguments"));
        assert!(s.contains("send [--workspace <repo>/<slug>] <label> <message...>"));
    }

    #[test]
    fn parses_config_set_literal() {
        let a = parse(&["config", "set", "branch_prefix", "bakedbean"]).unwrap();
        match a {
            CliAction::ConfigSet {
                key,
                source: ValueSource::Literal(v),
            } => {
                assert_eq!(key, "branch_prefix");
                assert_eq!(v, "bakedbean");
            }
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn parses_config_set_file_reference() {
        let a = parse(&["config", "set", "custom_instructions", "@/tmp/foo.md"]).unwrap();
        match a {
            CliAction::ConfigSet {
                key,
                source: ValueSource::File(p),
            } => {
                assert_eq!(key, "custom_instructions");
                assert_eq!(p, std::path::PathBuf::from("/tmp/foo.md"));
            }
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn rejects_unknown_setting_key() {
        assert!(parse(&["config", "set", "nope", "val"]).is_err());
        assert!(parse(&["config", "get", "nope"]).is_err());
    }

    #[test]
    fn unknown_setting_key_is_tagged_config_usage() {
        match parse(&["config", "set", "nope", "x"]) {
            Err(Error::Usage {
                group: Some("config"),
                msg,
            }) => {
                assert_eq!(msg, "unknown setting key: nope");
            }
            other => panic!("expected config-tagged Usage, got {other:?}"),
        }
        // get and edit forms too
        assert!(matches!(
            parse(&["config", "get", "nope"]),
            Err(Error::Usage {
                group: Some("config"),
                ..
            })
        ));
        assert!(matches!(
            parse(&["config", "edit", "nope"]),
            Err(Error::Usage {
                group: Some("config"),
                ..
            })
        ));
    }

    #[test]
    fn accepts_usage_graph_window() {
        assert!(known_setting_key("usage_graph_window"));
    }

    #[test]
    fn usage_window_validate_accepts_canonical_tokens() {
        assert_eq!(usage_window_validate_and_normalize("24h").unwrap(), "24h");
        assert_eq!(usage_window_validate_and_normalize("1w").unwrap(), "1w");
        assert_eq!(usage_window_validate_and_normalize("1mo").unwrap(), "1mo");
    }

    #[test]
    fn usage_window_validate_trims_whitespace() {
        assert_eq!(usage_window_validate_and_normalize(" 1w\n").unwrap(), "1w");
    }

    #[test]
    fn usage_window_validate_rejects_garbage() {
        assert!(usage_window_validate_and_normalize("week").is_err());
        assert!(usage_window_validate_and_normalize("").is_err());
        assert!(usage_window_validate_and_normalize("1d").is_err());
    }

    #[test]
    fn accepts_diff_cmd() {
        assert!(known_setting_key("diff_cmd"));
    }

    #[test]
    fn accepts_lazygit_cmd() {
        assert!(known_setting_key("lazygit_cmd"));
    }

    #[test]
    fn accepts_chronox_cmd() {
        assert!(known_setting_key("chronox_cmd"));
    }

    #[test]
    fn accepts_mcp_mirror() {
        assert!(known_setting_key("mcp_mirror"));
    }

    #[test]
    fn accepts_remote_control_settings() {
        assert!(known_setting_key("remote_control"));
        assert!(known_setting_key("remote_control_sandbox"));
    }

    #[test]
    fn parses_repo_set_prefix() {
        let a = parse(&["repo", "set-prefix", "myrepo", "bakedbean"]).unwrap();
        match a {
            CliAction::RepoSetPrefix { name, prefix } => {
                assert_eq!(name, "myrepo");
                assert_eq!(prefix, "bakedbean");
            }
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn parses_repo_set_setup_literal() {
        let a = parse(&["repo", "set-setup", "demo", "bun install"]).unwrap();
        match a {
            CliAction::RepoSetSetup {
                name,
                source: ValueSource::Literal(v),
            } => {
                assert_eq!(name, "demo");
                assert_eq!(v, "bun install");
            }
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn parses_repo_set_setup_file_reference() {
        let a = parse(&["repo", "set-setup", "demo", "@./setup.sh"]).unwrap();
        match a {
            CliAction::RepoSetSetup {
                name,
                source: ValueSource::File(p),
            } => {
                assert_eq!(name, "demo");
                assert_eq!(p, std::path::PathBuf::from("./setup.sh"));
            }
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn parses_repo_set_archive_literal() {
        let a = parse(&["repo", "set-archive", "demo", "rm -rf node_modules"]).unwrap();
        match a {
            CliAction::RepoSetArchive {
                name,
                source: ValueSource::Literal(v),
            } => {
                assert_eq!(name, "demo");
                assert_eq!(v, "rm -rf node_modules");
            }
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn parses_repo_edit_setup_and_edit_archive() {
        match parse(&["repo", "edit-setup", "demo"]).unwrap() {
            CliAction::RepoEditSetup { name } => assert_eq!(name, "demo"),
            _ => panic!("wrong action"),
        }
        match parse(&["repo", "edit-archive", "demo"]).unwrap() {
            CliAction::RepoEditArchive { name } => assert_eq!(name, "demo"),
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn config_set_accepts_pinned_commands_key() {
        let a = parse(&["config", "set", "pinned_commands", "/feedback"]).unwrap();
        match a {
            CliAction::ConfigSet { key, .. } => assert_eq!(key, "pinned_commands"),
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_set_pinned_commands_literal() {
        let a = parse(&["repo", "set-pinned-commands", "demo", "PR=/pull-request"]).unwrap();
        match a {
            CliAction::RepoSetPinnedCommands {
                name,
                source: ValueSource::Literal(v),
            } => {
                assert_eq!(name, "demo");
                assert_eq!(v, "PR=/pull-request");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_set_pinned_commands_at_file() {
        let a = parse(&["repo", "set-pinned-commands", "demo", "@./pinned.txt"]).unwrap();
        match a {
            CliAction::RepoSetPinnedCommands {
                name,
                source: ValueSource::File(p),
            } => {
                assert_eq!(name, "demo");
                assert_eq!(p, std::path::PathBuf::from("./pinned.txt"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_edit_pinned_commands() {
        match parse(&["repo", "edit-pinned-commands", "demo"]).unwrap() {
            CliAction::RepoEditPinnedCommands { name } => assert_eq!(name, "demo"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_set_related_repos_literal() {
        let a = parse(&["repo", "set-related-repos", "backend", "frontend,marketing"]).unwrap();
        match a {
            CliAction::RepoSetRelatedRepos { name, source } => {
                assert_eq!(name, "backend");
                assert!(matches!(source, ValueSource::Literal(ref s) if s == "frontend,marketing"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_set_related_repos_at_file() {
        let a = parse(&["repo", "set-related-repos", "backend", "@./related.txt"]).unwrap();
        match a {
            CliAction::RepoSetRelatedRepos { source, .. } => {
                assert!(matches!(source, ValueSource::File(_)));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_repo_set_name() {
        let a = parse(&["repo", "set-name", "myrepo", "my-new-name"]).unwrap();
        match a {
            CliAction::RepoSetName { name, new_name } => {
                assert_eq!(name, "myrepo");
                assert_eq!(new_name, "my-new-name");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_edit_related_repos() {
        match parse(&["repo", "edit-related-repos", "backend"]).unwrap() {
            CliAction::RepoEditRelatedRepos { name } => assert_eq!(name, "backend"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_remote_list_no_args() {
        match parse(&["remote"]).unwrap() {
            CliAction::RemoteList => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_remote_run_with_name() {
        match parse(&["remote", "ebenmini"]).unwrap() {
            CliAction::RemoteRun { name } => assert_eq!(name, "ebenmini"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn accepts_remotes_setting_key() {
        assert!(known_setting_key("remotes"));
    }

    #[test]
    fn accepts_shared_hosts_setting_key() {
        assert!(known_setting_key("shared_hosts"));
    }

    #[test]
    fn parses_shared_list_json() {
        match parse(&["shared", "list", "--json"]).unwrap() {
            CliAction::SharedList { json } => assert!(json),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_shared_list_without_json() {
        match parse(&["shared", "list"]).unwrap() {
            CliAction::SharedList { json } => assert!(!json),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_shared_list_rejects_unknown_arg() {
        assert!(parse(&["shared", "list", "--bogus"]).is_err());
    }

    #[test]
    fn parses_shared_rejects_unknown_subcommand() {
        assert!(parse(&["shared", "bogus"]).is_err());
        assert!(parse(&["shared"]).is_err());
    }

    #[test]
    fn parses_repo_set_base_branch_literal() {
        match parse(&["repo", "set-base-branch", "demo", "origin/main"]).unwrap() {
            CliAction::RepoSetBaseBranch { name, value } => {
                assert_eq!(name, "demo");
                assert_eq!(value, "origin/main");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_create_minimal() {
        match parse(&["workspace", "create", "backend"]).unwrap() {
            CliAction::WorkspaceCreate {
                repo,
                name,
                yolo,
                shared,
                agent: None,
                prompt,
            } => {
                assert_eq!(repo, "backend");
                assert!(name.is_none());
                assert!(!yolo);
                assert!(!shared);
                assert!(prompt.is_none());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_create_with_name_and_yolo() {
        match parse(&[
            "workspace",
            "create",
            "backend",
            "--name",
            "add-widgets",
            "--yolo",
        ])
        .unwrap()
        {
            CliAction::WorkspaceCreate {
                repo,
                name,
                yolo,
                shared,
                agent: None,
                prompt: None,
            } => {
                assert_eq!(repo, "backend");
                assert_eq!(name.as_deref(), Some("add-widgets"));
                assert!(yolo);
                assert!(!shared);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// The phone flow: one command creates the workspace AND seeds its
    /// agent, so the only thing typed on a phone keyboard is the prompt.
    #[test]
    fn parses_workspace_create_with_prompt() {
        match parse(&[
            "workspace",
            "create",
            "backend",
            "--prompt",
            "fix the flaky tests",
        ])
        .unwrap()
        {
            CliAction::WorkspaceCreate { repo, prompt, .. } => {
                assert_eq!(repo, "backend");
                assert_eq!(prompt.as_deref(), Some("fix the flaky tests"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// `--prompt` composes with every other create flag — the phone flow
    /// still wants `--yolo` and an explicit `--name` sometimes.
    #[test]
    fn parses_workspace_create_prompt_alongside_other_flags() {
        match parse(&[
            "workspace",
            "create",
            "backend",
            "--name",
            "flaky-tests",
            "--yolo",
            "--agent",
            "claude",
            "--prompt",
            "fix the flaky tests",
        ])
        .unwrap()
        {
            CliAction::WorkspaceCreate {
                repo,
                name,
                yolo,
                shared,
                agent,
                prompt,
            } => {
                assert_eq!(repo, "backend");
                assert_eq!(name.as_deref(), Some("flaky-tests"));
                assert!(yolo);
                assert!(!shared);
                assert_eq!(agent.as_deref(), Some("claude"));
                assert_eq!(prompt.as_deref(), Some("fix the flaky tests"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// A bare `--prompt` must be a usage error rather than silently
    /// creating an unseeded workspace the sender believes is running.
    #[test]
    fn parses_workspace_create_rejects_prompt_without_value() {
        assert!(parse(&["workspace", "create", "backend", "--prompt"]).is_err());
    }

    #[test]
    fn parses_workspace_create_with_shared() {
        let a = parse(&["workspace", "create", "myrepo", "--shared"]).unwrap();
        match a {
            CliAction::WorkspaceCreate { repo, shared, .. } => {
                assert_eq!(repo, "myrepo");
                assert!(shared);
            }
            other => panic!("wrong action: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_create_rejects_unknown_arg() {
        assert!(parse(&["workspace", "create", "backend", "--bogus"]).is_err());
    }

    /// The recovery hint has to be pasteable verbatim, so it must carry the
    /// real prompt — not a placeholder — and survive the two things that
    /// routinely break a hand-built command line: spaces in a repo name and
    /// arbitrary text in a prompt.
    #[test]
    fn retry_hint_resends_the_actual_prompt() {
        let hint = retry_send_hint("backend", "add-widgets", "fix the flaky tests");
        assert!(
            hint.contains("fix the flaky tests"),
            "must carry the real prompt, not a placeholder: {hint}"
        );
        assert!(
            !hint.contains("\"...\""),
            "a placeholder makes the hint unusable: {hint}"
        );
        assert!(hint.contains("--workspace"), "{hint}");
        assert!(hint.contains("primary"), "{hint}");
    }

    #[test]
    fn retry_hint_quotes_spaces_and_metacharacters() {
        // Repo names may contain spaces — `resolve_workspace_spec` splits on
        // the LAST slash for exactly this reason.
        let spaced = retry_send_hint("meals backend", "add-widgets", "do it");
        assert!(
            spaced.contains("'meals backend/add-widgets'"),
            "a spaced repo name must stay one argument: {spaced}"
        );

        // A prompt is arbitrary text; quotes and shell metacharacters in it
        // must not escape into the command.
        let nasty = retry_send_hint("r", "w", "it's $HOME; rm -rf /");
        let parsed = shlex::split(&nasty).expect("hint must be valid shell");
        assert_eq!(
            parsed.last().map(String::as_str),
            Some("it's $HOME; rm -rf /"),
            "the prompt must round-trip as a single literal argument: {nasty}"
        );
        assert_eq!(
            parsed,
            vec![
                "wsx",
                "agent",
                "send",
                "--workspace",
                "r/w",
                "primary",
                "it's $HOME; rm -rf /"
            ],
            "the hint must parse as exactly the intended argv"
        );
    }

    fn init_git_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        r(&["init", "-q", "-b", "main"]);
        r(&["config", "user.email", "t@e"]);
        r(&["config", "user.name", "t"]);
        r(&["commit", "--allow-empty", "-q", "-m", "init"]);
        dir
    }

    /// `--prompt` must queue against the workspace it just created, aimed at
    /// the primary agent seeded at birth. This is the whole phone flow: the
    /// dashboard spawns that agent on demand when it drains the inbox, so a
    /// message on the wrong target (or no message at all) is a workspace that
    /// silently never starts.
    #[tokio::test]
    async fn workspace_create_with_prompt_queues_it_to_the_new_primary() {
        use crate::config::Dirs;
        use crate::data::store::Store;
        use crate::test_support::EnvGuard;

        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        let repo_dir = init_git_repo();
        {
            let store = Store::open(&dirs.db_path()).unwrap();
            crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
                .await
                .unwrap();
        }

        let mut env = EnvGuard::new();
        // Point the "is a TUI running" check at an empty scratch dir so the
        // no-dashboard warning path is deterministic regardless of whether
        // this process is itself running under a live wsx dashboard.
        env.set("XDG_RUNTIME_DIR", tmp.path());
        env.remove("WSX_AGENT_INSTANCE_ID");

        run_cli(
            CliAction::WorkspaceCreate {
                repo: "demo".to_string(),
                name: Some("seeded".to_string()),
                yolo: false,
                shared: false,
                agent: None,
                prompt: Some("fix the flaky tests".to_string()),
            },
            &dirs,
        )
        .await
        .unwrap();

        let store = Store::open(&dirs.db_path()).unwrap();
        let ws = store
            .repos()
            .unwrap()
            .into_iter()
            .flat_map(|r| store.workspaces(r.id).unwrap())
            .find(|w| w.name == "seeded")
            .expect("workspace must exist");
        let primary = store
            .primary_instance_id(ws.id)
            .unwrap()
            .expect("create seeds a primary agent at birth");

        let queued = store.undelivered_messages().unwrap();
        assert_eq!(queued.len(), 1, "exactly one seeded prompt");
        assert_eq!(queued[0].workspace_id, ws.id);
        assert_eq!(
            queued[0].target_agent_id, primary,
            "must target the new workspace's primary agent"
        );
        assert_eq!(queued[0].body, "fix the flaky tests");
    }

    /// Without `--prompt`, create must not invent an inbox message —
    /// otherwise every plain `workspace create` would wake an agent.
    #[tokio::test]
    async fn workspace_create_without_prompt_queues_nothing() {
        use crate::config::Dirs;
        use crate::data::store::Store;
        use crate::test_support::EnvGuard;

        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::for_test(tmp.path());
        let repo_dir = init_git_repo();
        {
            let store = Store::open(&dirs.db_path()).unwrap();
            crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
                .await
                .unwrap();
        }

        let mut env = EnvGuard::new();
        env.set("XDG_RUNTIME_DIR", tmp.path());
        env.remove("WSX_AGENT_INSTANCE_ID");

        run_cli(
            CliAction::WorkspaceCreate {
                repo: "demo".to_string(),
                name: Some("quiet".to_string()),
                yolo: false,
                shared: false,
                agent: None,
                prompt: None,
            },
            &dirs,
        )
        .await
        .unwrap();

        let store = Store::open(&dirs.db_path()).unwrap();
        assert!(
            store.undelivered_messages().unwrap().is_empty(),
            "a create without --prompt must leave the inbox untouched"
        );
    }

    use crate::pty::session::AgentKind;

    fn parent_ws(yolo: bool, agent: AgentKind) -> crate::data::store::Workspace {
        use crate::data::store::{RepoId, SetupStatus, Workspace, WorkspaceId, WorkspaceState};
        Workspace {
            id: WorkspaceId(1),
            repo_id: RepoId(1),
            name: "parent".into(),
            branch: "x/parent".into(),
            worktree_path: std::path::PathBuf::from("/tmp/p"),
            state: WorkspaceState::Ready,
            setup_status: SetupStatus::Ok,
            created_at: 0,
            yolo,
            agent,
            shared: false,
        }
    }

    #[test]
    fn create_flags_without_parent_keep_todays_defaults() {
        assert_eq!(
            effective_create_flags(false, None, None, AgentKind::Claude),
            (false, AgentKind::Claude)
        );
        assert_eq!(
            effective_create_flags(true, Some("pi"), None, AgentKind::Claude),
            (true, AgentKind::Pi)
        );
    }

    #[test]
    fn create_flags_without_parent_fall_back_to_coding_agent_setting() {
        assert_eq!(
            effective_create_flags(false, None, None, AgentKind::Codex),
            (false, AgentKind::Codex)
        );
    }

    #[test]
    fn create_flags_inherit_yolo_and_agent_from_parent() {
        let parent = parent_ws(true, AgentKind::Pi);
        assert_eq!(
            effective_create_flags(false, None, Some(&parent), AgentKind::Claude),
            (true, AgentKind::Pi)
        );
    }

    #[test]
    fn create_flags_parent_agent_beats_coding_agent_setting() {
        let parent = parent_ws(false, AgentKind::Pi);
        assert_eq!(
            effective_create_flags(false, None, Some(&parent), AgentKind::Codex),
            (false, AgentKind::Pi)
        );
    }

    #[test]
    fn create_flags_explicit_agent_beats_parent() {
        let parent = parent_ws(false, AgentKind::Pi);
        assert_eq!(
            effective_create_flags(false, Some("codex"), Some(&parent), AgentKind::Claude),
            (false, AgentKind::Codex)
        );
    }

    #[test]
    fn create_flags_explicit_yolo_ors_with_parent() {
        let parent = parent_ws(false, AgentKind::Claude);
        assert_eq!(
            effective_create_flags(true, None, Some(&parent), AgentKind::Claude),
            (true, AgentKind::Claude)
        );
    }

    #[test]
    fn create_flags_non_yolo_claude_parent_matches_defaults() {
        let parent = parent_ws(false, AgentKind::Claude);
        assert_eq!(
            effective_create_flags(false, None, Some(&parent), AgentKind::Claude),
            (false, AgentKind::Claude)
        );
    }

    #[test]
    fn parses_workspace_list_no_filter() {
        match parse(&["workspace", "list"]).unwrap() {
            CliAction::WorkspaceList { repo } => assert!(repo.is_none()),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_list_with_repo_filter() {
        match parse(&["workspace", "list", "backend"]).unwrap() {
            CliAction::WorkspaceList { repo } => assert_eq!(repo.as_deref(), Some("backend")),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_path() {
        match parse(&["workspace", "path", "backend", "add-widgets"]).unwrap() {
            CliAction::WorkspacePath { repo, name } => {
                assert_eq!(repo, "backend");
                assert_eq!(name, "add-widgets");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_rename() {
        match parse(&["workspace", "rename", "backend", "old-slug", "new-slug"]).unwrap() {
            CliAction::WorkspaceRename {
                repo,
                name,
                new_name,
            } => {
                assert_eq!(repo, "backend");
                assert_eq!(name, "old-slug");
                assert_eq!(new_name, "new-slug");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_archive_minimal() {
        match parse(&["workspace", "archive", "backend", "add-widgets"]).unwrap() {
            CliAction::WorkspaceArchive {
                repo,
                name,
                keep_worktree,
                force_delete_branch,
            } => {
                assert_eq!(repo, "backend");
                assert_eq!(name, "add-widgets");
                assert!(!keep_worktree);
                assert!(!force_delete_branch);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_archive_with_flags() {
        match parse(&[
            "workspace",
            "archive",
            "backend",
            "add-widgets",
            "--keep-worktree",
            "--force-delete-branch",
        ])
        .unwrap()
        {
            CliAction::WorkspaceArchive {
                keep_worktree,
                force_delete_branch,
                ..
            } => {
                assert!(keep_worktree);
                assert!(force_delete_branch);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_share() {
        match parse(&["workspace", "share", "backend", "add-widgets"]).unwrap() {
            CliAction::WorkspaceShare { repo, name, shared } => {
                assert_eq!(repo, "backend");
                assert_eq!(name, "add-widgets");
                assert!(shared);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_unshare() {
        match parse(&["workspace", "unshare", "backend", "add-widgets"]).unwrap() {
            CliAction::WorkspaceShare { repo, name, shared } => {
                assert_eq!(repo, "backend");
                assert_eq!(name, "add-widgets");
                assert!(!shared);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_rejects_unknown_subcommand() {
        assert!(parse(&["workspace", "bogus"]).is_err());
    }

    #[test]
    fn parses_setup_install_skill() {
        match parse(&["setup", "install-skill"]).unwrap() {
            CliAction::SetupInstallSkill => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_setup_rejects_unknown_subcommand() {
        assert!(parse(&["setup", "bogus"]).is_err());
        assert!(parse(&["setup"]).is_err());
    }

    #[test]
    fn parses_repo_set_base_branch_empty_value() {
        match parse(&["repo", "set-base-branch", "demo", ""]).unwrap() {
            CliAction::RepoSetBaseBranch { name, value } => {
                assert_eq!(name, "demo");
                assert_eq!(value, "");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn detail_bar_config_seed_returns_pretty_default_when_empty() {
        let seed = super::detail_bar_config_seed_for_empty();
        // Sanity: round-trips to default config.
        let parsed: crate::config::detail_bar_config::DetailBarConfig =
            serde_json::from_str(&seed).unwrap();
        assert_eq!(
            parsed,
            crate::config::detail_bar_config::DetailBarConfig::default()
        );
        // Pretty-printed: contains newlines.
        assert!(seed.contains('\n'));
    }

    #[test]
    fn detail_bar_config_validate_rejects_malformed() {
        let result = super::detail_bar_config_validate_and_normalize("{not json");
        assert!(result.is_err());
    }

    #[test]
    fn detail_bar_config_validate_clamps_out_of_range() {
        let json = r#"{"height": {"percent": 200}}"#;
        let normalized = super::detail_bar_config_validate_and_normalize(json).unwrap();
        let parsed: crate::config::detail_bar_config::DetailBarConfig =
            serde_json::from_str(&normalized).unwrap();
        assert_eq!(parsed.height.percent, 80);
    }

    #[test]
    fn detail_bar_config_validate_accepts_partial() {
        let json = r#"{"visible": false}"#;
        let normalized = super::detail_bar_config_validate_and_normalize(json).unwrap();
        let parsed: crate::config::detail_bar_config::DetailBarConfig =
            serde_json::from_str(&normalized).unwrap();
        assert!(!parsed.visible);
        assert_eq!(parsed.height.percent, 30);
    }

    #[test]
    fn detail_bar_config_default_seed_round_trips() {
        use crate::config::detail_bar_config::DetailBarConfig;
        let seed =
            serde_json::to_string_pretty(&DetailBarConfig::default()).expect("serialize default");
        let parsed: DetailBarConfig =
            serde_json::from_str(&seed).expect("seed must parse with new schema");
        assert_eq!(parsed, DetailBarConfig::default());
        // Spot-check: the new shape uses `containers`, not `sections`.
        assert!(seed.contains("\"containers\""));
        assert!(!seed.contains("\"sections\""));
    }

    #[test]
    fn process_doctrine_is_a_known_setting() {
        assert!(known_setting_key("process_doctrine"));
    }

    #[test]
    fn parses_agent_send_joins_prompt() {
        match parse(&["agent", "send", "claude#2", "hello", "there"]).unwrap() {
            CliAction::AgentSend {
                target,
                prompt,
                workspace,
            } => {
                assert_eq!(target, "claude#2");
                assert_eq!(prompt, "hello there");
                assert_eq!(workspace, None, "no flag → current workspace");
            }
            other => panic!("expected AgentSend, got {other:?}"),
        }
    }

    #[test]
    fn parses_agent_list_and_add() {
        assert!(matches!(
            parse(&["agent", "list"]).unwrap(),
            CliAction::AgentList
        ));
        assert!(matches!(
            parse(&["agent", "add", "codex"]).unwrap(),
            CliAction::AgentAdd { .. }
        ));
        assert!(parse(&["agent", "add", "bogus"]).is_err());
    }

    #[test]
    fn detail_bar_config_validate_truncates_too_many_containers() {
        let raw = serde_json::json!({
            "containers": [
                ["a"], ["b"], ["c"], ["d"], ["e"], ["f"]
            ]
        })
        .to_string();
        let normalized = super::detail_bar_config_validate_and_normalize(&raw)
            .expect("valid JSON should normalize");
        // Truncation happens inside sanitize(); the normalized blob
        // should round-trip to exactly 4 containers.
        let parsed: crate::config::detail_bar_config::DetailBarConfig =
            serde_json::from_str(&normalized).expect("re-parse normalized");
        assert_eq!(parsed.containers.len(), 4);
    }

    #[test]
    fn report_cli_error_formats_usage_block() {
        let e = Error::Usage {
            group: Some("agent"),
            msg: "agent send needs <label> <message...>".into(),
        };
        let s = report_cli_error(&e);
        assert!(s.starts_with("error: agent send needs"));
        assert!(s.contains("send [--workspace <repo>/<slug>] <label> <message...>"));
    }

    #[test]
    fn report_cli_error_falls_back_for_other_errors() {
        let e = Error::UserInput("unknown setting key: nope".into());
        let s = report_cli_error(&e);
        assert!(s.contains("unknown setting key: nope"));
    }

    #[test]
    fn unknown_subcommand_messages_are_clean() {
        // No Debug-formatted Option (`None` / `Some("..")`) leaking into user text.
        let missing = match parse(&["workspace"]) {
            Err(e) => e.to_string(),
            _ => panic!("expected error"),
        };
        assert_eq!(missing, "missing workspace command");
        let unknown = match parse(&["workspace", "bogus"]) {
            Err(e) => e.to_string(),
            _ => panic!("expected error"),
        };
        assert_eq!(unknown, "unknown workspace command: bogus");
        assert!(!missing.contains("None"));
        assert!(!unknown.contains("Some("));
    }

    #[test]
    fn registry_matches_dispatched_groups() {
        // Every group the dispatcher accepts must have a help entry, and every
        // help entry must be a real group. Update BOTH when adding a command group.
        let dispatched = [
            "workspace",
            "agent",
            "repo",
            "config",
            "remote",
            "shared",
            "setup",
            "status",
            "recap",
            "waybar",
            "menubar",
        ];
        let registry: Vec<&str> = GROUPS.iter().map(|g| g.name).collect();
        for d in dispatched {
            assert!(
                registry.contains(&d),
                "group `{d}` dispatched but missing from GROUPS"
            );
        }
        for r in &registry {
            assert!(
                dispatched.contains(r),
                "group `{r}` in GROUPS but not dispatched"
            );
        }
    }

    #[test]
    fn parses_status_set_with_message() {
        let a = parse_args(
            [
                "wsx",
                "status",
                "set",
                "blocked",
                "--message",
                "need a decision",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        )
        .unwrap();
        match a {
            CliAction::StatusSet { state, message } => {
                assert_eq!(state, "blocked");
                assert_eq!(message.as_deref(), Some("need a decision"));
            }
            other => panic!("expected StatusSet, got {other:?}"),
        }
    }

    #[test]
    fn parses_status_set_without_message() {
        let a = parse_args(
            ["wsx", "status", "set", "working"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
        .unwrap();
        match a {
            CliAction::StatusSet { state, message } => {
                assert_eq!(state, "working");
                assert_eq!(message, None);
            }
            other => panic!("expected StatusSet, got {other:?}"),
        }
    }

    #[test]
    fn parses_status_clear_and_from_hook() {
        assert!(matches!(
            parse_args(
                ["wsx", "status", "clear"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            )
            .unwrap(),
            CliAction::StatusClear
        ));
        assert!(matches!(
            parse_args(
                ["wsx", "status", "from-hook"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            )
            .unwrap(),
            CliAction::StatusFromHook { agent: None }
        ));
        match parse_args(
            ["wsx", "status", "from-hook", "--agent", "claude"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
        .unwrap()
        {
            CliAction::StatusFromHook { agent } => assert_eq!(agent.as_deref(), Some("claude")),
            other => panic!("expected StatusFromHook, got {other:?}"),
        }
    }

    #[test]
    fn parse_status_from_notify_captures_agent_and_payload() {
        match parse_args(
            [
                "wsx",
                "status",
                "from-notify",
                "--agent",
                "codex",
                "{\"type\":\"agent-turn-complete\"}",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        )
        .unwrap()
        {
            CliAction::StatusFromNotify { agent, payload } => {
                assert_eq!(agent.as_deref(), Some("codex"));
                assert_eq!(
                    payload.as_deref(),
                    Some("{\"type\":\"agent-turn-complete\"}")
                );
            }
            other => panic!("expected StatusFromNotify, got {other:?}"),
        }
    }

    #[test]
    fn parse_status_from_notify_with_no_args_is_all_none() {
        match parse_args(
            ["wsx", "status", "from-notify"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
        .unwrap()
        {
            CliAction::StatusFromNotify { agent, payload } => {
                assert!(agent.is_none());
                assert!(payload.is_none());
            }
            other => panic!("expected StatusFromNotify, got {other:?}"),
        }
    }

    #[test]
    fn status_set_message_without_value_is_usage_error() {
        let err = parse_args(
            ["wsx", "status", "set", "working", "--message"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Usage { .. }), "got {err:?}");
    }

    #[test]
    fn status_from_hook_agent_without_value_is_usage_error() {
        let err = parse_args(
            ["wsx", "status", "from-hook", "--agent"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Usage { .. }), "got {err:?}");
    }

    #[test]
    fn parses_recap_set_with_all_flags() {
        let a = parse(&[
            "recap",
            "set",
            "--goal",
            "fix auth",
            "--state",
            "tests failing",
            "--next",
            "debug",
        ])
        .unwrap();
        match a {
            CliAction::RecapSet {
                goal, state, next, ..
            } => {
                assert_eq!(goal.as_deref(), Some("fix auth"));
                assert_eq!(state.as_deref(), Some("tests failing"));
                assert_eq!(next.as_deref(), Some("debug"));
            }
            other => panic!("expected RecapSet, got {other:?}"),
        }
    }

    #[test]
    fn parses_recap_set_partial() {
        let a = parse(&["recap", "set", "--state", "tests green"]).unwrap();
        match a {
            CliAction::RecapSet {
                goal, state, next, ..
            } => {
                assert_eq!(goal, None);
                assert_eq!(state.as_deref(), Some("tests green"));
                assert_eq!(next, None);
            }
            other => panic!("expected RecapSet, got {other:?}"),
        }
    }

    #[test]
    fn recap_set_requires_at_least_one_flag() {
        assert!(parse(&["recap", "set"]).is_err());
    }

    #[test]
    fn recap_set_rejects_unknown_flag() {
        assert!(parse(&["recap", "set", "--bogus", "x"]).is_err());
    }

    #[test]
    fn parses_recap_set_short_forms() {
        let a = parse(&[
            "recap",
            "set",
            "--goal-short",
            "Audit V2 invoices, CV-04964",
            "--state-short",
            "3/12 done",
            "--next-short",
            "fix drift calc",
        ])
        .unwrap();
        match a {
            CliAction::RecapSet {
                goal,
                goal_short,
                state_short,
                next_short,
                ..
            } => {
                assert_eq!(goal, None);
                assert_eq!(goal_short.as_deref(), Some("Audit V2 invoices, CV-04964"));
                assert_eq!(state_short.as_deref(), Some("3/12 done"));
                assert_eq!(next_short.as_deref(), Some("fix drift calc"));
            }
            other => panic!("expected RecapSet, got {other:?}"),
        }
    }

    #[test]
    fn recap_set_short_flag_alone_satisfies_at_least_one() {
        assert!(parse(&["recap", "set", "--goal-short", "x"]).is_ok());
    }

    #[test]
    fn parses_recap_show_and_clear() {
        assert!(matches!(
            parse(&["recap", "show"]).unwrap(),
            CliAction::RecapShow
        ));
        assert!(matches!(
            parse(&["recap", "clear"]).unwrap(),
            CliAction::RecapClear
        ));
    }

    #[test]
    fn parses_waybar_commands() {
        assert!(matches!(
            parse(&["waybar", "status"]),
            Ok(CliAction::WaybarStatus)
        ));
        assert!(matches!(
            parse(&["waybar", "menu"]),
            Ok(CliAction::WaybarMenu)
        ));
        match parse(&["waybar", "jump", "meals backend", "api-fix"]) {
            Ok(CliAction::WaybarJump { repo, slug }) => {
                assert_eq!(repo, "meals backend");
                assert_eq!(slug, "api-fix");
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(parse(&["waybar", "jump", "onlyrepo"]).is_err());
        assert!(parse(&["waybar", "bogus"]).is_err());
        assert!(parse(&["waybar"]).is_err());
    }

    #[test]
    fn parses_waybar_menu_entries_and_refresh_prs() {
        assert!(matches!(
            parse(&["waybar", "menu-entries"]),
            Ok(CliAction::WaybarMenuEntries)
        ));
        assert!(matches!(
            parse(&["waybar", "menu-entries", "--json"]),
            Ok(CliAction::WaybarMenuEntries)
        ));
        assert!(matches!(
            parse(&["waybar", "refresh-prs"]),
            Ok(CliAction::WaybarRefreshPrs)
        ));
    }

    #[test]
    fn parses_setup_waybar() {
        assert!(matches!(
            parse(&["setup", "waybar"]),
            Ok(CliAction::SetupWaybar)
        ));
    }

    #[test]
    fn waybar_group_help_renders() {
        let h = render_group_help("waybar");
        assert!(h.contains("wsx waybar —"));
        assert!(h.contains("status"));
        assert!(h.contains("jump <repo> <slug>"));
    }

    #[test]
    fn parses_menubar_commands() {
        assert!(matches!(
            parse(&["menubar", "plugin"]),
            Ok(CliAction::MenubarPlugin)
        ));
        match parse(&["menubar", "jump", "meals backend", "api-fix"]) {
            Ok(CliAction::MenubarJump { repo, slug }) => {
                assert_eq!(repo, "meals backend");
                assert_eq!(slug, "api-fix");
            }
            other => panic!("{other:?}"),
        }
        match parse(&["menubar", "copy-path", "r", "s"]) {
            Ok(CliAction::MenubarCopyPath { repo, slug }) => {
                assert_eq!(repo, "r");
                assert_eq!(slug, "s");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse(&["menubar", "refresh"]),
            Ok(CliAction::MenubarRefresh)
        ));
        assert!(parse(&["menubar", "jump", "onlyrepo"]).is_err());
        assert!(parse(&["menubar", "bogus"]).is_err());
        assert!(parse(&["menubar"]).is_err());
    }

    #[test]
    fn parses_setup_menubar() {
        assert!(matches!(
            parse(&["setup", "menubar"]),
            Ok(CliAction::SetupMenubar)
        ));
    }

    #[test]
    fn menubar_group_help_renders() {
        let h = render_group_help("menubar");
        assert!(h.contains("wsx menubar —"));
        assert!(h.contains("plugin"));
        assert!(h.contains("copy-path"));
    }
}
